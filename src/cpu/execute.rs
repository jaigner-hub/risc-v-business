use anyhow::Result;
use crate::cpu::{Cpu, PrivMode, decode::Instruction};
use crate::cpu::mmu::AccessType;

/// Sign-extend a u64 value from bit `n` (0-indexed).
/// Used for *W variants: sext(val, 31) sign-extends from bit 31.
#[inline(always)]
fn sext(val: u64, bit: u32) -> u64 {
    let shift = 63 - bit;
    (((val << shift) as i64) >> shift) as u64
}

pub fn execute(cpu: &mut Cpu, inst: Instruction) -> Result<()> {
    let pc = cpu.pc;

    // Most instructions advance pc by 4. Branches and jumps set pc directly.
    let mut next_pc = pc.wrapping_add(4);

    // MPRV (mstatus[17]): loads/stores use MPP-mode translation when set. Priv §3.1.6.3.
    // Instruction fetch is NOT affected by MPRV.
    let ls_mode = if (cpu.csr.mstatus >> 17) & 1 == 1 {
        match (cpu.csr.mstatus >> 11) & 3 { 0 => PrivMode::U, 1 => PrivMode::S, _ => PrivMode::M }
    } else {
        cpu.mode
    };

    match inst {
        // --- R-type arithmetic ---
        Instruction::Add  { rd, rs1, rs2 } => {
            let v = cpu.reg(rs1).wrapping_add(cpu.reg(rs2));
            cpu.set_reg(rd, v);
        },
        Instruction::Sub  { rd, rs1, rs2 } => {
            let v = cpu.reg(rs1).wrapping_sub(cpu.reg(rs2));
            cpu.set_reg(rd, v);
        },
        Instruction::Sll  { rd, rs1, rs2 } => {
            let v = cpu.reg(rs1) << (cpu.reg(rs2) & 0x3f);
            cpu.set_reg(rd, v);
        },
        Instruction::Slt  { rd, rs1, rs2 } => {
            let v = ((cpu.reg(rs1) as i64) < (cpu.reg(rs2) as i64)) as u64;
            cpu.set_reg(rd, v);
        },
        Instruction::Sltu { rd, rs1, rs2 } => {
            let v = (cpu.reg(rs1) < cpu.reg(rs2)) as u64;
            cpu.set_reg(rd, v);
        },
        Instruction::Xor  { rd, rs1, rs2 } => { cpu.set_reg(rd, cpu.reg(rs1) ^ cpu.reg(rs2)); },
        Instruction::Srl  { rd, rs1, rs2 } => {
            let v = cpu.reg(rs1) >> (cpu.reg(rs2) & 0x3f);
            cpu.set_reg(rd, v);
        },
        Instruction::Sra  { rd, rs1, rs2 } => {
            let v = ((cpu.reg(rs1) as i64) >> (cpu.reg(rs2) & 0x3f)) as u64;
            cpu.set_reg(rd, v);
        },
        Instruction::Or   { rd, rs1, rs2 } => { cpu.set_reg(rd, cpu.reg(rs1) | cpu.reg(rs2)); },
        Instruction::And  { rd, rs1, rs2 } => { cpu.set_reg(rd, cpu.reg(rs1) & cpu.reg(rs2)); },

        // --- RV64I W-variants: operate on lower 32 bits, sign-extend result to 64 ---
        // Spec: Unprivileged §5.2 — result is sign-extended from bit 31
        Instruction::Addw { rd, rs1, rs2 } => {
            let v = sext(cpu.reg(rs1).wrapping_add(cpu.reg(rs2)), 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Subw { rd, rs1, rs2 } => {
            let v = sext(cpu.reg(rs1).wrapping_sub(cpu.reg(rs2)), 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Sllw { rd, rs1, rs2 } => {
            let v = sext(cpu.reg(rs1) << (cpu.reg(rs2) & 0x1f), 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Srlw { rd, rs1, rs2 } => {
            let v = sext((cpu.reg(rs1) as u32 >> (cpu.reg(rs2) & 0x1f)) as u64, 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Sraw { rd, rs1, rs2 } => {
            let v = sext(((cpu.reg(rs1) as i32) >> (cpu.reg(rs2) & 0x1f)) as u64, 31);
            cpu.set_reg(rd, v);
        },

        // --- I-type arithmetic ---
        Instruction::Addi  { rd, rs1, imm } => {
            let v = cpu.reg(rs1).wrapping_add(imm as u64);
            cpu.set_reg(rd, v);
        },
        Instruction::Slti  { rd, rs1, imm } => {
            let v = ((cpu.reg(rs1) as i64) < imm) as u64;
            cpu.set_reg(rd, v);
        },
        Instruction::Sltiu { rd, rs1, imm } => {
            let v = (cpu.reg(rs1) < imm as u64) as u64;
            cpu.set_reg(rd, v);
        },
        Instruction::Xori  { rd, rs1, imm } => { cpu.set_reg(rd, cpu.reg(rs1) ^ imm as u64); },
        Instruction::Ori   { rd, rs1, imm } => { cpu.set_reg(rd, cpu.reg(rs1) | imm as u64); },
        Instruction::Andi  { rd, rs1, imm } => { cpu.set_reg(rd, cpu.reg(rs1) & imm as u64); },
        Instruction::Slli  { rd, rs1, shamt } => { cpu.set_reg(rd, cpu.reg(rs1) << shamt); },
        Instruction::Srli  { rd, rs1, shamt } => { cpu.set_reg(rd, cpu.reg(rs1) >> shamt); },
        Instruction::Srai  { rd, rs1, shamt } => {
            let v = ((cpu.reg(rs1) as i64) >> shamt) as u64;
            cpu.set_reg(rd, v);
        },

        // --- RV64I IW-variants ---
        Instruction::Addiw { rd, rs1, imm } => {
            let v = sext(cpu.reg(rs1).wrapping_add(imm as u64), 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Slliw { rd, rs1, shamt } => {
            let v = sext(cpu.reg(rs1) << shamt, 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Srliw { rd, rs1, shamt } => {
            let v = sext((cpu.reg(rs1) as u32 >> shamt) as u64, 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Sraiw { rd, rs1, shamt } => {
            let v = sext(((cpu.reg(rs1) as i32) >> shamt) as u64, 31);
            cpu.set_reg(rd, v);
        },

        // --- Loads ---
        // All addresses go through the MMU. Page faults deliver a trap and skip set_reg.
        Instruction::Lb  { rd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 1) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.set_reg(rd, sext(v, 7)),
                }
            }
        },
        Instruction::Lh  { rd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 2) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.set_reg(rd, sext(v, 15)),
                }
            }
        },
        Instruction::Lw  { rd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 4) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.set_reg(rd, sext(v, 31)),
                }
            }
        },
        Instruction::Ld  { rd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 8) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.set_reg(rd, v),
                }
            }
        },
        Instruction::Lbu { rd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 1) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.set_reg(rd, v),
                }
            }
        },
        Instruction::Lhu { rd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 2) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.set_reg(rd, v),
                }
            }
        },
        Instruction::Lwu { rd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 4) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.set_reg(rd, v),
                }
            }
        },

        // --- Stores ---
        Instruction::Sb { rs1, rs2, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.store(pa, 1, cpu.reg(rs2)) {
                    Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
                    Ok(_)  => {}
                }
            }
        },
        Instruction::Sh { rs1, rs2, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.store(pa, 2, cpu.reg(rs2)) {
                    Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
                    Ok(_)  => {}
                }
            }
        },
        Instruction::Sw { rs1, rs2, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.store(pa, 4, cpu.reg(rs2)) {
                    Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
                    Ok(_)  => {}
                }
            }
        },
        Instruction::Sd { rs1, rs2, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.store(pa, 8, cpu.reg(rs2)) {
                    Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
                    Ok(_)  => {}
                }
            }
        },

        // --- Branches ---
        // Misaligned branch targets trap before the instruction commits. Spec: Priv §3.1.15
        Instruction::Beq  { rs1, rs2, imm } => {
            if cpu.reg(rs1) == cpu.reg(rs2) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x3 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bne  { rs1, rs2, imm } => {
            if cpu.reg(rs1) != cpu.reg(rs2) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x3 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Blt  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) < (cpu.reg(rs2) as i64) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x3 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bge  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) >= (cpu.reg(rs2) as i64) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x3 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bltu { rs1, rs2, imm } => {
            if cpu.reg(rs1) < cpu.reg(rs2) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x3 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bgeu { rs1, rs2, imm } => {
            if cpu.reg(rs1) >= cpu.reg(rs2) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x3 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },

        // --- Jumps ---
        // JAL: rd = pc+4, pc = pc + imm. Spec: Unprivileged §2.5
        Instruction::Jal { rd, imm } => {
            let target = pc.wrapping_add(imm as u64);
            if target & 0x3 != 0 {
                cpu.deliver_trap(0, target); next_pc = cpu.pc;
            } else {
                cpu.set_reg(rd, next_pc);
                next_pc = target;
            }
        },
        // JALR: rd = pc+4, pc = (rs1 + imm) & ~1. Spec: Unprivileged §2.5
        Instruction::Jalr { rd, rs1, imm } => {
            let target = cpu.reg(rs1).wrapping_add(imm as u64) & !1;
            if target & 0x3 != 0 {
                cpu.deliver_trap(0, target); next_pc = cpu.pc;
            } else {
                cpu.set_reg(rd, next_pc);
                next_pc = target;
            }
        },

        // --- Upper immediate ---
        // LUI: rd = SignExt(imm[31:12] << 12). imm already has lower 12 bits = 0.
        Instruction::Lui   { rd, imm } => { cpu.set_reg(rd, imm as u64); },
        // AUIPC: rd = pc + SignExt(imm[31:12] << 12).
        Instruction::Auipc { rd, imm } => { cpu.set_reg(rd, pc.wrapping_add(imm as u64)); },

        // --- System ---
        Instruction::Fence => { /* NOP in Phase 1 — no memory-ordering concerns */ },
        Instruction::Wfi   => { /* NOP — no interrupts in Phase 1 */ },

        // ECALL: cause depends on current privilege mode. Priv §3.3.1 Table 3.6
        // deliver_trap() handles medeleg delegation automatically.
        Instruction::Ecall => {
            let cause = match cpu.mode { PrivMode::U => 8, PrivMode::S => 9, PrivMode::M => 11 };
            cpu.deliver_trap(cause, 0);
            next_pc = cpu.pc;
        },
        // EBREAK: cause=3 (breakpoint). Uses deliver_trap so medeleg applies. Priv §3.3.1
        Instruction::Ebreak => {
            cpu.deliver_trap(3, cpu.pc);
            next_pc = cpu.pc;
        },
        // MRET: restore privilege to MPP, then jump to mepc. Priv §3.3.2
        Instruction::Mret => {
            let mpp = (cpu.csr.mstatus >> 11) & 3;
            cpu.mode = match mpp { 0 => PrivMode::U, 1 => PrivMode::S, _ => PrivMode::M };
            cpu.csr.mret(); // MIE←MPIE, MPIE←1, MPP←U
            // If returning to non-M mode, clear MPRV. Priv §3.3.2.
            if mpp != 3 { cpu.csr.mstatus &= !(1u64 << 17); }
            next_pc = cpu.csr.mepc;
        },
        // SRET: restore privilege to SPP, jump to sepc. Illegal when in S-mode with mstatus.TSR=1. Priv §3.1.6.5
        Instruction::Sret => {
            if cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 22) & 1 != 0 {
                cpu.deliver_trap(2, 0x10200073); // TSR=1 in S-mode: illegal instruction
                next_pc = cpu.pc;
            } else {
                let spp = (cpu.csr.mstatus >> 8) & 1;
                cpu.mode = if spp == 1 { PrivMode::S } else { PrivMode::U };
                cpu.csr.mstatus &= !(1u64 << 17); // SRET always clears MPRV (SPP can't be M). Priv §3.3.2
                cpu.csr.s_ret(); // SIE←SPIE, SPIE←1, SPP←0
                next_pc = cpu.csr.sepc;
            }
        },
        Instruction::SfenceVma => {
            // TVM=mstatus[20]: S-mode execution of SFENCE.VMA with TVM=1 is illegal. Priv §3.1.6.5.
            if cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0);
                next_pc = cpu.pc;
            } else {
                cpu.mmu.flush();
            }
        },
        // --- Zicsr (CSR access) ---
        // Spec: Unprivileged §9.1. Each CSRR* atomically reads csr into rd, then
        // writes a new value back. The exact write source/operation differs per
        // mnemonic. If rd=x0, the read is suppressed for csrrw[i] (no side effects);
        // for the others, if rs1/uimm is 0, the write is suppressed.
        Instruction::Csrrw { rd, rs1, csr } => {
            // TVM: S-mode satp access with TVM=1 is illegal. Priv §3.1.6.5.
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let new_val = cpu.reg(rs1);
                let old = if rd != 0 { cpu.csr_read(csr) } else { 0 };
                cpu.csr_write(csr, new_val);
                cpu.set_reg(rd, old);
            }
        },
        Instruction::Csrrs { rd, rs1, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = cpu.csr_read(csr);
                let mask = cpu.reg(rs1);
                if rs1 != 0 { cpu.csr_write(csr, old | mask); }
                cpu.set_reg(rd, old);
            }
        },
        Instruction::Csrrc { rd, rs1, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = cpu.csr_read(csr);
                let mask = cpu.reg(rs1);
                if rs1 != 0 { cpu.csr_write(csr, old & !mask); }
                cpu.set_reg(rd, old);
            }
        },
        Instruction::Csrrwi { rd, uimm, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = if rd != 0 { cpu.csr_read(csr) } else { 0 };
                cpu.csr_write(csr, uimm as u64);
                cpu.set_reg(rd, old);
            }
        },
        Instruction::Csrrsi { rd, uimm, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = cpu.csr_read(csr);
                if uimm != 0 { cpu.csr_write(csr, old | uimm as u64); }
                cpu.set_reg(rd, old);
            }
        },
        Instruction::Csrrci { rd, uimm, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = cpu.csr_read(csr);
                if uimm != 0 { cpu.csr_write(csr, old & !(uimm as u64)); }
                cpu.set_reg(rd, old);
            }
        },
        // --- M extension: integer multiply/divide ---
        // Spec: Unprivileged §7
        Instruction::Mul { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i64 as i128;
            let b = cpu.reg(rs2) as i64 as i128;
            cpu.set_reg(rd, (a * b) as u64);
        },
        Instruction::Mulh { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i64 as i128;
            let b = cpu.reg(rs2) as i64 as i128;
            cpu.set_reg(rd, ((a * b) >> 64) as u64);
        },
        Instruction::Mulhsu { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i64 as i128;
            let b = cpu.reg(rs2) as u128 as i128;
            cpu.set_reg(rd, ((a * b) >> 64) as u64);
        },
        Instruction::Mulhu { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as u128;
            let b = cpu.reg(rs2) as u128;
            cpu.set_reg(rd, ((a * b) >> 64) as u64);
        },
        Instruction::Div { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i64;
            let b = cpu.reg(rs2) as i64;
            let v = if b == 0 {
                -1i64 as u64
            } else if a == i64::MIN && b == -1 {
                i64::MIN as u64
            } else {
                (a / b) as u64
            };
            cpu.set_reg(rd, v);
        },
        Instruction::Divu { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1);
            let b = cpu.reg(rs2);
            cpu.set_reg(rd, if b == 0 { u64::MAX } else { a / b });
        },
        Instruction::Rem { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i64;
            let b = cpu.reg(rs2) as i64;
            let v = if b == 0 {
                a as u64
            } else if a == i64::MIN && b == -1 {
                0
            } else {
                (a % b) as u64
            };
            cpu.set_reg(rd, v);
        },
        Instruction::Remu { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1);
            let b = cpu.reg(rs2);
            cpu.set_reg(rd, if b == 0 { a } else { a % b });
        },
        // M extension W-variants: operate on lower 32 bits, sign-extend result from bit 31
        Instruction::Mulw { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i32;
            let b = cpu.reg(rs2) as i32;
            cpu.set_reg(rd, sext(a.wrapping_mul(b) as u64, 31));
        },
        Instruction::Divw { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i32;
            let b = cpu.reg(rs2) as i32;
            let v = if b == 0 {
                -1i64 as u64
            } else if a == i32::MIN && b == -1 {
                sext(i32::MIN as u64, 31)
            } else {
                sext((a / b) as u64, 31)
            };
            cpu.set_reg(rd, v);
        },
        Instruction::Divuw { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as u32;
            let b = cpu.reg(rs2) as u32;
            let v = if b == 0 { u64::MAX } else { sext((a / b) as u64, 31) };
            cpu.set_reg(rd, v);
        },
        Instruction::Remw { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as i32;
            let b = cpu.reg(rs2) as i32;
            let v = if b == 0 {
                sext(a as u64, 31)
            } else if a == i32::MIN && b == -1 {
                0
            } else {
                sext((a % b) as u64, 31)
            };
            cpu.set_reg(rd, v);
        },
        Instruction::Remuw { rd, rs1, rs2 } => {
            let a = cpu.reg(rs1) as u32;
            let b = cpu.reg(rs2) as u32;
            let v = if b == 0 { sext(a as u64, 31) } else { sext((a % b) as u64, 31) };
            cpu.set_reg(rd, v);
        },
        // --- A extension: atomics ---
        // Spec: Unprivileged §8. Single-hart: aq/rl ordering is trivially satisfied.
        Instruction::LrD { rd, rs1 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 8) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => { cpu.set_reg(rd, v); cpu.reservation = Some(pa); }
                }
            }
        },
        Instruction::LrW { rd, rs1 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 4) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => { cpu.set_reg(rd, sext(v, 31)); cpu.reservation = Some(pa); }
                }
            }
        },
        Instruction::ScD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    if cpu.reservation == Some(pa) {
                        cpu.bus.store(pa, 8, cpu.reg(rs2)).ok();
                        cpu.set_reg(rd, 0);
                    } else {
                        cpu.set_reg(rd, 1);
                    }
                    cpu.reservation = None;
                }
            }
        },
        Instruction::ScW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    if cpu.reservation == Some(pa) {
                        cpu.bus.store(pa, 4, cpu.reg(rs2)).ok();
                        cpu.set_reg(rd, 0);
                    } else {
                        cpu.set_reg(rd, 1);
                    }
                    cpu.reservation = None;
                }
            }
        },
        Instruction::AmoswapD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = cpu.bus.load(pa, 8).unwrap_or(0); cpu.bus.store(pa, 8, cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoswapW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31); cpu.bus.store(pa, 4, cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoaddD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = cpu.bus.load(pa, 8).unwrap_or(0); cpu.bus.store(pa, 8, old.wrapping_add(cpu.reg(rs2))).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoaddW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31); cpu.bus.store(pa, 4, old.wrapping_add(cpu.reg(rs2))).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoxorD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = cpu.bus.load(pa, 8).unwrap_or(0); cpu.bus.store(pa, 8, old ^ cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoxorW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31); cpu.bus.store(pa, 4, old ^ cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoandD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = cpu.bus.load(pa, 8).unwrap_or(0); cpu.bus.store(pa, 8, old & cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoandW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31); cpu.bus.store(pa, 4, old & cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoorD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = cpu.bus.load(pa, 8).unwrap_or(0); cpu.bus.store(pa, 8, old | cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmoorW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => { let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31); cpu.bus.store(pa, 4, old | cpu.reg(rs2)).ok(); cpu.set_reg(rd, old); }
            }
        },
        Instruction::AmominD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = cpu.bus.load(pa, 8).unwrap_or(0);
                    let new = if (old as i64) < (cpu.reg(rs2) as i64) { old } else { cpu.reg(rs2) };
                    cpu.bus.store(pa, 8, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
        Instruction::AmominW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31);
                    let rs2v = cpu.reg(rs2);
                    let new = if (old as i32) < (rs2v as i32) { old } else { sext(rs2v, 31) };
                    cpu.bus.store(pa, 4, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
        Instruction::AmomaxD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = cpu.bus.load(pa, 8).unwrap_or(0);
                    let new = if (old as i64) > (cpu.reg(rs2) as i64) { old } else { cpu.reg(rs2) };
                    cpu.bus.store(pa, 8, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
        Instruction::AmomaxW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31);
                    let rs2v = cpu.reg(rs2);
                    let new = if (old as i32) > (rs2v as i32) { old } else { sext(rs2v, 31) };
                    cpu.bus.store(pa, 4, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
        Instruction::AmominuD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = cpu.bus.load(pa, 8).unwrap_or(0);
                    let new = old.min(cpu.reg(rs2));
                    cpu.bus.store(pa, 8, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
        Instruction::AmominuW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31);
                    let rs2v = cpu.reg(rs2) as u32;
                    let new = if (old as u32) < rs2v { old } else { rs2v as u64 };
                    cpu.bus.store(pa, 4, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
        Instruction::AmomaxuD { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = cpu.bus.load(pa, 8).unwrap_or(0);
                    let new = old.max(cpu.reg(rs2));
                    cpu.bus.store(pa, 8, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
        Instruction::AmomaxuW { rd, rs1, rs2 } => {
            let va = cpu.reg(rs1);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => {
                    let old = sext(cpu.bus.load(pa, 4).unwrap_or(0), 31);
                    let rs2v = cpu.reg(rs2) as u32;
                    let new = if (old as u32) > rs2v { old } else { rs2v as u64 };
                    cpu.bus.store(pa, 4, new).ok();
                    cpu.set_reg(rd, old);
                }
            }
        },
    }

    cpu.pc = next_pc;
    Ok(())
}

// CSR addresses used by the Phase 1 trap path. Spec: Privileged §2.2.
const CSR_MTVEC:  u16 = 0x305;
const CSR_MEPC:   u16 = 0x341;
const CSR_MCAUSE: u16 = 0x342;
const CSR_MTVAL:  u16 = 0x343;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bus::Bus, cpu::{Cpu, decode::Instruction}};

    fn cpu_with_ram(size: usize) -> Cpu {
        Cpu::new(Bus::new(size, 0x8000_0000), 0x8000_0000, false)
    }

    #[test] fn add_wraps() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, u64::MAX);
        c.set_reg(2, 1);
        execute(&mut c, Instruction::Add { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), 0);
    }

    #[test] fn slt_signed() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, (-1i64) as u64);
        c.set_reg(2, 0);
        execute(&mut c, Instruction::Slt { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), 1); // -1 < 0 signed
    }

    #[test] fn sltu_unsigned() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, (-1i64) as u64); // 0xFFFF... is large unsigned
        c.set_reg(2, 0);
        execute(&mut c, Instruction::Sltu { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), 0); // 0xFFFF > 0 unsigned
    }

    #[test] fn addw_sign_extends() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, 0x0000_0000_8000_0000); // bit 31 set
        c.set_reg(2, 0);
        execute(&mut c, Instruction::Addw { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), 0xFFFF_FFFF_8000_0000u64); // sign-extended
    }

    #[test] fn jal_sets_rd_and_pc() {
        let mut c = cpu_with_ram(64);
        c.pc = 0x8000_0000;
        execute(&mut c, Instruction::Jal { rd: 1, imm: 8 }).unwrap();
        assert_eq!(c.reg(1), 0x8000_0004); // return address
        assert_eq!(c.pc, 0x8000_0008);
    }

    #[test] fn lui_loads_upper() {
        let mut c = cpu_with_ram(64);
        c.pc = 0x8000_0000;
        execute(&mut c, Instruction::Lui { rd: 1, imm: 0x12345000 }).unwrap();
        assert_eq!(c.reg(1), 0x12345000);
    }

    #[test] fn mul_lower64() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, (-1i64) as u64);  // 0xFFFF...FFFF
        c.set_reg(2, 2u64);
        execute(&mut c, Instruction::Mul { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), (-2i64) as u64); // lower 64 of -1*2 = -2
    }

    #[test] fn mulh_upper64() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, i64::MIN as u64);    // -2^63
        c.set_reg(2, (-1i64) as u64);     // -1
        execute(&mut c, Instruction::Mulh { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        // (-2^63) * (-1) = 2^63; upper 64 bits of 2^63 as i128 = 0
        assert_eq!(c.reg(3), 0);
    }

    #[test] fn div_signed_by_zero() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, 7u64);
        c.set_reg(2, 0u64);
        execute(&mut c, Instruction::Div { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), (-1i64) as u64); // quotient = -1 on div-by-zero
    }

    #[test] fn div_signed_overflow() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, i64::MIN as u64);    // -2^63
        c.set_reg(2, (-1i64) as u64);     // -1
        execute(&mut c, Instruction::Div { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), i64::MIN as u64); // quotient = -2^63 on overflow
    }

    #[test] fn divu_by_zero() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, 42u64);
        c.set_reg(2, 0u64);
        execute(&mut c, Instruction::Divu { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), u64::MAX);
    }

    #[test] fn rem_signed_by_zero() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, 7u64);
        c.set_reg(2, 0u64);
        execute(&mut c, Instruction::Rem { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), 7u64); // remainder = dividend on div-by-zero
    }

    #[test] fn mulw_sign_extends() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, 0x0000_0000_8000_0001u64);
        c.set_reg(2, 0x0000_0000_0000_0002u64);
        execute(&mut c, Instruction::Mulw { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        // lower 32: 0x8000_0001 * 2 = 0x1_0000_0002; truncated to 32 bits = 0x0000_0002; sign-extend = 2
        assert_eq!(c.reg(3), 2u64);
    }

    #[test] fn lr_sc_success() {
        let mut c = cpu_with_ram(1024);
        // Store a value in RAM at offset 0 (address 0x8000_0000)
        c.bus.store(0x8000_0000, 8, 0xABCD_1234u64).unwrap();
        c.set_reg(1, 0x8000_0000); // rs1 = address
        // LR.D: load from (rs1), set reservation
        execute(&mut c, Instruction::LrD { rd: 2, rs1: 1 }).unwrap();
        assert_eq!(c.reg(2), 0xABCD_1234);
        assert_eq!(c.reservation, Some(0x8000_0000));
        // SC.D: reservation matches → store succeeds, rd=0
        c.set_reg(3, 0xDEAD_BEEF);
        execute(&mut c, Instruction::ScD { rd: 4, rs1: 1, rs2: 3 }).unwrap();
        assert_eq!(c.reg(4), 0); // success
        assert_eq!(c.reservation, None);
        assert_eq!(c.bus.load(0x8000_0000, 8).unwrap(), 0xDEAD_BEEF);
    }

    #[test] fn sc_failure_clears_reservation() {
        let mut c = cpu_with_ram(1024);
        c.reservation = None; // no reservation
        c.set_reg(1, 0x8000_0000);
        c.set_reg(2, 42);
        execute(&mut c, Instruction::ScD { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), 1); // failure
        assert_eq!(c.reservation, None);
    }

    #[test] fn amoadd_d() {
        let mut c = cpu_with_ram(1024);
        c.bus.store(0x8000_0000, 8, 10u64).unwrap();
        c.set_reg(1, 0x8000_0000);
        c.set_reg(2, 5u64);
        execute(&mut c, Instruction::AmoaddD { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), 10); // old value
        assert_eq!(c.bus.load(0x8000_0000, 8).unwrap(), 15); // new value
    }

    #[test] fn amomin_d_signed() {
        let mut c = cpu_with_ram(1024);
        c.bus.store(0x8000_0000, 8, (-3i64) as u64).unwrap();
        c.set_reg(1, 0x8000_0000);
        c.set_reg(2, (-5i64) as u64);
        execute(&mut c, Instruction::AmominD { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), (-3i64) as u64); // old
        assert_eq!(c.bus.load(0x8000_0000, 8).unwrap(), (-5i64) as u64); // min(-3,-5) = -5
    }
}
