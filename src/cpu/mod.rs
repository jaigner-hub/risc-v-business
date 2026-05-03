pub mod csr;
pub mod decode;
pub mod execute;

use crate::bus::Bus;
use anyhow::Result;
use csr::Csr;

pub mod mmu;
use mmu::AccessType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivMode { U = 0, S = 1, M = 3 }

pub struct Tracer {
    pub enabled: bool,
}

impl Tracer {
    pub fn new(enabled: bool) -> Self { Self { enabled } }

    pub fn trace_step(&self, pc: u64, raw: u32, mnemonic: &str, operands: &str,
                      reg_changes: &[(usize, u64, u64)]) {
        if !self.enabled { return; }
        let changes: String = reg_changes.iter()
            .map(|(r, old, new)| format!(" x{r}: {old:#018x} -> {new:#018x}"))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!("[{pc:#010x}] {raw:08x}  {mnemonic:<8} {operands:<24}{changes}");
    }
}

pub struct Cpu {
    pub regs:   [u64; 32],
    /// Floating-point registers (F/D extension). Stored as u64 to hold either
    /// a 32-bit single (NaN-boxed in upper bits) or a 64-bit double.
    /// Phase 5: only FLW/FLD/FSW/FSD use these — no FP arithmetic implemented.
    pub fregs: [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
    pub csr: Csr,
    pub reservation: Option<u64>,
    pub mode: PrivMode,
    pub mmu: mmu::Mmu,
    /// Size of the current instruction in bytes (4 for full, 2 for RVC).
    /// execute() reads this to compute link-register and next-PC values.
    pub inst_size: u64,
    /// Floating-point control and status register (CSR 0x003).
    /// Stub: not driven by FP arithmetic, but preserved across save/restore.
    pub fcsr: u32,
    /// JIT block cache invalidation flag. Set by execute arms that change
    /// address translation (satp write, sfence.vma) so the run loop can
    /// flush JitCache before continuing.
    pub jit_invalidate: bool,
}

// Compute trap PC per Priv §5.1.10: vectored mode dispatches interrupts to base + cause_code*4.
fn trap_pc(tvec: u64, cause: u64) -> u64 {
    let base = tvec & !0x3;
    let mode = tvec & 0x3;
    let is_interrupt = (cause >> 63) != 0;
    if is_interrupt && mode == 1 {
        base + ((cause & !(1u64 << 63)) * 4)
    } else {
        base
    }
}

impl Cpu {
    pub fn new(bus: Bus, entry: u64, trace: bool) -> Self {
        Self {
            regs: [0u64; 32],
            fregs: [0u64; 32],
            pc: entry,
            bus,
            tracer: Tracer::new(trace),
            csr: Csr::new(),
            reservation: None,
            mode: PrivMode::M,
            mmu: mmu::Mmu::new(),
            inst_size: 4,
            fcsr: 0,
            jit_invalidate: false,
        }
    }

    #[inline]
    pub fn csr_read(&self, addr: u16) -> u64 {
        match addr {
            0xC01 => self.bus.clint.mtime, // time: read CLINT mtime. Priv §3.2.1
            _ => self.csr.read(addr),
        }
    }

    #[inline]
    pub fn csr_write(&mut self, addr: u16, val: u64) {
        self.csr.write(addr, val);
    }

    /// Read register. x0 always returns 0.
    #[inline(always)]
    pub fn reg(&self, n: usize) -> u64 {
        assert!(n < 32, "register index out of range: {n}");
        if n == 0 { 0 } else { self.regs[n] }
    }

    /// Write register. Writes to x0 are silently ignored.
    #[inline(always)]
    pub fn set_reg(&mut self, n: usize, val: u64) {
        assert!(n < 32, "register index out of range: {n}");
        if n != 0 { self.regs[n] = val; }
    }

    pub fn deliver_trap(&mut self, cause: u64, tval: u64) {
        let is_interrupt = (cause >> 63) != 0;
        let cause_code   = cause & !(1u64 << 63);
        let deleg_reg    = if is_interrupt { self.csr.mideleg } else { self.csr.medeleg };
        let delegated    = cause_code < 64  // only codes [0,64) are delegatable per Priv §3.1.8
                           && (deleg_reg >> cause_code) & 1 == 1
                           && self.mode != PrivMode::M;

        if delegated {
            self.csr.s_trap_entry(self.mode == PrivMode::S);
            self.csr.sepc   = self.pc;
            self.csr.scause = cause;
            self.csr.stval  = tval;
            self.pc   = trap_pc(self.csr.stvec, cause);
            self.mode = PrivMode::S;
        } else {
            self.csr.trap_entry(self.mode as u64);
            self.csr.mepc   = self.pc;
            self.csr.mcause = cause;
            self.csr.mtval  = tval;
            self.pc   = trap_pc(self.csr.mtvec, cause);
            self.mode = PrivMode::M;
        }
    }

    /// Check for pending interrupts. Returns true if an interrupt was delivered.
    /// Priority order (highest first): MEI(11) > MSI(3) > MTI(7) > SEI(9) > SSI(1) > STI(5).
    pub fn check_interrupts(&mut self) -> bool {
        let pending = self.csr.mip & self.csr.mie;
        if pending == 0 { return false; }

        for &cause_code in &[11u64, 3, 7, 9, 1, 5] {
            if (pending >> cause_code) & 1 == 0 { continue; }

            let delegated = (self.csr.mideleg >> cause_code) & 1 == 1;
            let can_fire = match self.mode {
                PrivMode::M => !delegated && self.csr.mie_bit() == 1,
                PrivMode::S | PrivMode::U => {
                    if delegated {
                        (self.csr.mstatus >> 1) & 1 == 1  // SIE
                    } else {
                        true  // non-delegated M-mode interrupt preempts S/U
                    }
                }
            };

            if can_fire {
                let cause = (1u64 << 63) | cause_code;
                self.deliver_trap(cause, 0);
                return true;
            }
        }
        false
    }

    /// Fetch, decode, execute one instruction. Advances pc.
    /// Fetch faults deliver mcause=1; decode failures deliver mcause=2.
    /// Execute errors still propagate as Err.
    pub fn step(&mut self) -> Result<()> {
        use decode::{decode, decode_rvc};
        use execute::execute;

        // Check for pending interrupts before fetching the next instruction.
        if self.check_interrupts() {
            return Ok(());
        }

        let pc = self.pc;

        // RVC allows 2-byte aligned PCs; only odd addresses are illegal.
        if pc & 0x1 != 0 {
            self.deliver_trap(0, pc);
            return Ok(());
        }

        // Instruction fetch — translate VA → PA through MMU, then bus load. Priv §4.3.
        // Fetch low 16 bits first to detect RVC (16-bit) vs 32-bit without crossing page boundaries.
        let pa = match self.mmu.translate(
            &mut self.bus, self.csr.satp, self.mode, self.csr.mstatus, pc, AccessType::Fetch
        ) {
            Err(f) => { self.deliver_trap(f.cause, f.tval); return Ok(()); }
            Ok(pa) => pa,
        };

        // Fetch low 16 bits first; determines 16-bit vs 32-bit instruction.
        let lo = match self.bus.load(pa, 2) {
            Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
            Ok(v)  => v as u16,
        };

        let raw = if lo & 0x3 != 0x3 {
            // 16-bit RVC instruction: do not fetch upper 2 bytes.
            lo as u32
        } else {
            // 32-bit instruction: fetch upper 16 bits.
            let hi = match self.bus.load(pa + 2, 2) {
                Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
                Ok(v)  => v as u16,
            };
            (lo as u32) | ((hi as u32) << 16)
        };

        // Detect RVC (16-bit) vs full (32-bit) instruction.
        let is_rvc = raw & 0x3 != 0x3;

        // Decode — any error is an illegal instruction; raw bits go into mtval
        let inst = if is_rvc {
            self.inst_size = 2;
            match decode_rvc(raw as u16) {
                Ok(i) => i,
                Err(_) => { self.deliver_trap(2, (raw & 0xffff) as u64); return Ok(()); }
            }
        } else {
            self.inst_size = 4;
            match decode(raw) {
                Ok(i) => i,
                Err(_) => { self.deliver_trap(2, raw as u64); return Ok(()); }
            }
        };

        // Execute (tracing path or fast path)
        if self.tracer.enabled {
            let before = self.regs;
            let mnemonic = format!("{inst:?}");
            let short = mnemonic.split(' ').next().unwrap_or(&mnemonic).to_owned();
            execute(self, inst)?;
            let changes: Vec<(usize, u64, u64)> = (1..32)
                .filter(|&i| before[i] != self.regs[i])
                .map(|i| (i, before[i], self.regs[i]))
                .collect();
            self.tracer.trace_step(pc, raw, &short, "", &changes);
        } else {
            execute(self, inst)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;

    fn cpu() -> Cpu {
        Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false)
    }

    #[test] fn x0_always_zero() {
        let mut c = cpu();
        c.set_reg(0, 0xDEAD);
        assert_eq!(c.reg(0), 0);
    }

    #[test] fn reg_read_write() {
        let mut c = cpu();
        c.set_reg(5, 0xCAFE);
        assert_eq!(c.reg(5), 0xCAFE);
    }

    #[test]
    fn step_delivers_illegal_instruction_trap() {
        let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
        // Write mtvec = 0x8000_0010 (direct mode)
        c.csr.mtvec = 0x8000_0010;
        // Write an illegal instruction (0xDEAD_BEFF) at PC
        c.bus.store(0x8000_0000, 4, 0xDEAD_BEFFu64).unwrap();
        // step() should NOT return an error; it should deliver the trap
        c.step().unwrap();
        assert_eq!(c.pc, 0x8000_0010);         // jumped to mtvec
        assert_eq!(c.csr.mcause, 2);           // illegal instruction cause
        assert_eq!(c.csr.mtval, 0xDEAD_BEFF); // raw bits
        assert_eq!(c.csr.mepc, 0x8000_0000);  // PC of faulting instruction
    }

    #[test]
    fn step_delivers_fetch_fault_on_bad_address() {
        let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
        c.csr.mtvec = 0x8000_0010;
        // Point PC at an unmapped address (outside RAM)
        c.pc = 0x0000_0000;
        c.step().unwrap();
        assert_eq!(c.pc, 0x8000_0010);
        assert_eq!(c.csr.mcause, 1); // instruction access fault
        assert_eq!(c.csr.mtval, 0x0000_0000);
    }

    #[test]
    fn deliver_trap_routes_to_m_when_no_delegation() {
        let mut c = cpu();
        c.csr.mtvec = 0x8000_0100;
        c.csr.medeleg = 0; // nothing delegated
        c.deliver_trap(8, 0);
        assert_eq!(c.csr.mcause, 8);
        assert_eq!(c.pc, 0x8000_0100);
        assert_eq!(c.mode, PrivMode::M);
    }

    #[test]
    fn deliver_trap_routes_to_s_when_delegated() {
        let mut c = cpu();
        c.csr.stvec  = 0x8000_0200;
        c.csr.mtvec  = 0x8000_0100;
        c.csr.medeleg = 1 << 8; // delegate cause 8 (ecall from U-mode)
        c.mode = PrivMode::U;
        c.deliver_trap(8, 0xABCD);
        assert_eq!(c.csr.scause, 8);
        assert_eq!(c.csr.stval,  0xABCD);
        assert_eq!(c.pc,   0x8000_0200);
        assert_eq!(c.mode, PrivMode::S);
        // M-mode registers untouched
        assert_eq!(c.csr.mcause, 0);
        assert_eq!(c.csr.sepc, 0x8000_0000);
    }

    #[test]
    fn deliver_trap_m_mode_not_delegated_even_if_medeleg_set() {
        // Traps from M-mode are never delegated (Priv §3.1.8)
        let mut c = cpu();
        c.csr.mtvec   = 0x8000_0100;
        c.csr.medeleg = 1 << 11; // bit 11 = ecall from M-mode
        c.mode = PrivMode::M;
        c.deliver_trap(11, 0);
        assert_eq!(c.csr.mcause, 11);
        assert_eq!(c.pc, 0x8000_0100);
        assert_eq!(c.mode, PrivMode::M);
    }

    #[test]
    fn deliver_trap_s_mode_delegated_via_medeleg() {
        // Per Priv §3.1.8: traps from S-mode with medeleg[cause]=1 are handled by stvec.
        // S-mode is less privileged than M-mode, so delegation applies (mode != M).
        let mut c = cpu();
        c.csr.stvec   = 0x8000_0200;
        c.csr.mtvec   = 0x8000_0100;
        c.csr.medeleg = 1 << 9; // delegate S-mode ecall (cause=9) to S-mode
        c.mode = PrivMode::S;
        c.deliver_trap(9, 0);
        // Should go to stvec, not mtvec, since medeleg[9]=1 and current mode != M
        assert_eq!(c.csr.scause, 9);
        assert_eq!(c.pc, 0x8000_0200);
        assert_eq!(c.mode, PrivMode::S);
        assert_eq!(c.csr.mcause, 0); // M-mode registers untouched
    }

    #[test]
    fn deliver_trap_interrupt_uses_mideleg_not_medeleg() {
        let mut c = cpu();
        c.csr.stvec   = 0x8000_0200;
        c.csr.mtvec   = 0x8000_0100;
        // Delegate MTI (cause 7, interrupt) to S-mode via mideleg
        c.csr.mideleg = 1 << 7;
        c.csr.medeleg = 0; // medeleg NOT set for cause 7
        c.mode = PrivMode::S;
        // Deliver interrupt: cause = (1<<63) | 7
        c.deliver_trap((1u64 << 63) | 7, 0);
        // Should go to stvec (delegated via mideleg), not mtvec
        assert_eq!(c.mode, PrivMode::S);
        assert_eq!(c.csr.scause, (1u64 << 63) | 7);
        assert_eq!(c.csr.mcause, 0);
    }

    #[test]
    fn deliver_trap_vectored_mode_interrupt() {
        let mut c = cpu();
        // mtvec = base 0x8000_0000 | MODE=1 (vectored)
        c.csr.mtvec = 0x8000_0001;
        // Deliver MTI interrupt: cause = (1<<63)|7
        c.deliver_trap((1u64 << 63) | 7, 0);
        // vectored: PC = base + cause_code * 4 = 0x8000_0000 + 7*4 = 0x8000_001C
        assert_eq!(c.pc, 0x8000_001C);
        assert_eq!(c.csr.mcause, (1u64 << 63) | 7);
    }

    #[test]
    fn deliver_trap_vectored_mode_exception_uses_base() {
        let mut c = cpu();
        c.csr.mtvec = 0x8000_0001; // vectored mode
        c.deliver_trap(8, 0);       // exception (no bit 63): always goes to base
        assert_eq!(c.pc, 0x8000_0000); // base only, not base + 8*4
    }

    #[test]
    fn deliver_trap_s_mode_vectored_stvec() {
        let mut c = cpu();
        c.csr.stvec   = 0x9000_0001; // S-mode vectored (base=0x9000_0000, MODE=1)
        c.csr.mtvec   = 0x8000_0100;
        c.csr.mideleg = 1 << 5;      // delegate STI (cause 5) to S-mode
        c.mode = PrivMode::S;
        c.csr.mstatus = 1 << 1;      // SIE=1
        c.deliver_trap((1u64 << 63) | 5, 0); // STI interrupt
        assert_eq!(c.pc, 0x9000_0014); // 0x9000_0000 + 5*4 = 0x9000_0014
        assert_eq!(c.mode, PrivMode::S);
        assert_eq!(c.csr.scause, (1u64 << 63) | 5);
    }

    #[test]
    fn step_rvc_c_nop_advances_pc_by_2() {
        // C.NOP (0x0001) must advance PC by 2, not 4.
        let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
        c.csr.mtvec = 0x8000_0100;
        c.bus.store(0x8000_0000, 2, 0x0001u64).unwrap(); // C.NOP = 0x0001
        c.step().unwrap();
        assert_eq!(c.pc, 0x8000_0002); // RVC: advance by 2
        assert_eq!(c.csr.mcause, 0);   // no trap
    }

    #[test]
    fn step_32bit_at_odd_2byte_boundary_fetches_correctly() {
        // A 32-bit instruction at a 2-byte-aligned (but not 4-byte-aligned) address.
        // Tests that the 2+2 fetch reads both halves correctly.
        // ADDI x1, x0, 42  = 0x02A00093
        // At address 0x8000_0002 (2-byte aligned, not 4-byte aligned):
        let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0002, false);
        c.csr.mtvec = 0x8000_0100;
        // Write lower 16 bits at 0x8000_0002, upper 16 bits at 0x8000_0004
        c.bus.store(0x8000_0002, 2, 0x0093u64).unwrap(); // low half of ADDI
        c.bus.store(0x8000_0004, 2, 0x02A0u64).unwrap(); // high half of ADDI
        c.step().unwrap();
        assert_eq!(c.reg(1), 42);       // ADDI x1, x0, 42 executed correctly
        assert_eq!(c.pc, 0x8000_0006);  // advanced by 4
        assert_eq!(c.csr.mcause, 0);
    }

    #[test]
    fn wfi_advances_pc_and_does_not_trap() {
        let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
        c.csr.mtvec = 0x8000_0100;
        // WFI encoding: 0x10500073
        c.bus.store(0x8000_0000, 4, 0x10500073u64).unwrap();
        c.step().unwrap();
        assert_eq!(c.pc, 0x8000_0004); // advanced past WFI
        assert_eq!(c.csr.mcause, 0);   // no trap
    }

    #[test]
    fn check_interrupts_fires_mti_in_m_mode_with_mie_set() {
        let mut c = cpu();
        c.csr.mtvec   = 0x8000_0100; // direct mode
        c.csr.mie     = 1 << 7;      // MTI enable
        c.csr.mip     = 1 << 7;      // MTI pending
        c.csr.mstatus = 1 << 3;      // MIE=1
        c.mode = PrivMode::M;
        c.step().unwrap();            // step() calls check_interrupts() at top
        assert_eq!(c.csr.mcause, (1u64 << 63) | 7); // MTI interrupt cause
        assert_eq!(c.pc, 0x8000_0100);               // direct mode: goes to base
    }

    #[test]
    fn check_interrupts_does_not_fire_when_mie_clear() {
        let mut c = cpu();
        c.csr.mtvec   = 0x8000_0100;
        c.csr.mie     = 1 << 7;      // MTI enable
        c.csr.mip     = 1 << 7;      // MTI pending
        c.csr.mstatus = 0;            // MIE=0 — interrupts globally disabled
        c.mode = PrivMode::M;
        c.bus.store(0x8000_0000, 4, 0x00000013u64).unwrap(); // NOP
        c.step().unwrap();
        assert_eq!(c.csr.mcause, 0);  // no interrupt delivered
        assert_eq!(c.pc, 0x8000_0004); // NOP executed
    }

    #[test]
    fn check_interrupts_routes_delegated_sti_to_s_mode() {
        let mut c = cpu();
        c.csr.stvec   = 0x8000_0200;
        c.csr.mtvec   = 0x8000_0100;
        c.csr.mie     = 1 << 5;      // STIE enable (bit 5)
        c.csr.mip     = 1 << 5;      // STIP pending (bit 5)
        c.csr.mideleg = 1 << 5;      // delegate STI to S-mode
        c.csr.mstatus = 1 << 1;      // SIE=1 (bit 1)
        c.mode = PrivMode::S;
        c.step().unwrap();
        assert_eq!(c.csr.scause, (1u64 << 63) | 5); // STI interrupt cause
        assert_eq!(c.mode, PrivMode::S);             // stays in S-mode (delegated)
        assert_eq!(c.csr.mcause, 0);                 // M-mode untouched
    }

    #[test]
    fn satp_write_sets_jit_invalidate() {
        use crate::cpu::execute::execute;
        use crate::cpu::decode::Instruction;
        let mut c = cpu();
        c.jit_invalidate = false;
        // CSRW satp, x1 = CSRRW x0, satp(0x180), x1
        execute(&mut c, Instruction::Csrrw { rd: 0, rs1: 1, csr: 0x180 }).unwrap();
        assert!(c.jit_invalidate, "writing satp must set jit_invalidate");
    }

    #[test]
    fn sfence_sets_jit_invalidate() {
        use crate::cpu::execute::execute;
        use crate::cpu::decode::Instruction;
        let mut c = cpu();
        c.jit_invalidate = false;
        execute(&mut c, Instruction::SfenceVma).unwrap();
        assert!(c.jit_invalidate, "SFENCE.VMA must set jit_invalidate");
    }

    #[test]
    fn csrrwi_satp_sets_jit_invalidate() {
        use crate::cpu::execute::execute;
        use crate::cpu::decode::Instruction;
        let mut c = cpu();
        c.jit_invalidate = false;
        // csrwi satp, 0 — should still flush even with uimm=0
        execute(&mut c, Instruction::Csrrwi { rd: 0, uimm: 0, csr: 0x180 }).unwrap();
        assert!(c.jit_invalidate, "csrwi satp must set jit_invalidate even with uimm=0");
    }

    #[test]
    fn csrrs_satp_rs1_zero_does_not_invalidate() {
        use crate::cpu::execute::execute;
        use crate::cpu::decode::Instruction;
        let mut c = cpu();
        c.jit_invalidate = false;
        // CSRRS x0, satp, x0 — read-only, must NOT set jit_invalidate
        execute(&mut c, Instruction::Csrrs { rd: 0, rs1: 0, csr: 0x180 }).unwrap();
        assert!(!c.jit_invalidate, "Csrrs with rs1=0 must not set jit_invalidate");
    }
}
