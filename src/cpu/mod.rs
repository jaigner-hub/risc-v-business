pub mod csr;
pub mod decode;
pub mod execute;

use crate::bus::Bus;
use anyhow::Result;
use csr::Csr;

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
    regs:   [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
    pub csr: Csr,
    pub reservation: Option<u64>,
}

impl Cpu {
    pub fn new(bus: Bus, entry: u64, trace: bool) -> Self {
        Self {
            regs: [0u64; 32],
            pc: entry,
            bus,
            tracer: Tracer::new(trace),
            csr: Csr::new(),
            reservation: None,
        }
    }

    #[inline]
    pub fn csr_read(&self, addr: u16) -> u64 {
        self.csr.read(addr)
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
        self.csr.trap_entry();
        self.csr.mepc   = self.pc;
        self.csr.mcause = cause;
        self.csr.mtval  = tval;
        self.pc = self.csr.mtvec & !0b11;
    }

    /// Fetch, decode, execute one instruction. Advances pc.
    /// Fetch faults deliver mcause=1; decode failures deliver mcause=2.
    /// Execute errors still propagate as Err.
    pub fn step(&mut self) -> Result<()> {
        use decode::decode;
        use execute::execute;

        let pc = self.pc;

        // Instruction fetch — bus error → mcause=1 (instruction access fault)
        let raw = match self.bus.load(pc, 4) {
            Ok(v) => v as u32,
            Err(_) => {
                self.deliver_trap(1, pc);
                return Ok(());
            }
        };

        // Decode — any error is an illegal instruction; raw bits go into mtval
        let inst = match decode(raw) {
            Ok(i) => i,
            Err(_) => {
                self.deliver_trap(2, raw as u64);
                return Ok(());
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
}
