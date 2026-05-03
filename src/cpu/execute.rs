use anyhow::{anyhow, Result};
use crate::cpu::{Cpu, decode::Instruction};

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
        // All sign-extend except LBU/LHU/LWU. Spec: Unprivileged §2.6
        Instruction::Lb  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = sext(cpu.bus.load(addr, 1)?, 7);
            cpu.set_reg(rd, v);
        },
        Instruction::Lh  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = sext(cpu.bus.load(addr, 2)?, 15);
            cpu.set_reg(rd, v);
        },
        Instruction::Lw  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = sext(cpu.bus.load(addr, 4)?, 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Ld  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = cpu.bus.load(addr, 8)?;
            cpu.set_reg(rd, v);
        },
        Instruction::Lbu { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            cpu.set_reg(rd, cpu.bus.load(addr, 1)?);
        },
        Instruction::Lhu { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            cpu.set_reg(rd, cpu.bus.load(addr, 2)?);
        },
        Instruction::Lwu { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            cpu.set_reg(rd, cpu.bus.load(addr, 4)?); // zero-extended (load already gives u64)
        },

        // --- Stores ---
        Instruction::Sb { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 1, cpu.reg(rs2))?;
        },
        Instruction::Sh { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 2, cpu.reg(rs2))?;
        },
        Instruction::Sw { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 4, cpu.reg(rs2))?;
        },
        Instruction::Sd { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 8, cpu.reg(rs2))?;
        },

        // --- Branches ---
        Instruction::Beq  { rs1, rs2, imm } => {
            if cpu.reg(rs1) == cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bne  { rs1, rs2, imm } => {
            if cpu.reg(rs1) != cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Blt  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) < (cpu.reg(rs2) as i64) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bge  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) >= (cpu.reg(rs2) as i64) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bltu { rs1, rs2, imm } => {
            if cpu.reg(rs1) < cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bgeu { rs1, rs2, imm } => {
            if cpu.reg(rs1) >= cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },

        // --- Jumps ---
        // JAL: rd = pc+4, pc = pc + imm. Spec: Unprivileged §2.5
        Instruction::Jal { rd, imm } => {
            cpu.set_reg(rd, next_pc);
            next_pc = pc.wrapping_add(imm as u64);
        },
        // JALR: rd = pc+4, pc = (rs1 + imm) & ~1. Spec: Unprivileged §2.5
        Instruction::Jalr { rd, rs1, imm } => {
            let target = cpu.reg(rs1).wrapping_add(imm as u64) & !1;
            cpu.set_reg(rd, next_pc);
            next_pc = target;
        },

        // --- Upper immediate ---
        // LUI: rd = SignExt(imm[31:12] << 12). imm already has lower 12 bits = 0.
        Instruction::Lui   { rd, imm } => { cpu.set_reg(rd, imm as u64); },
        // AUIPC: rd = pc + SignExt(imm[31:12] << 12).
        Instruction::Auipc { rd, imm } => { cpu.set_reg(rd, pc.wrapping_add(imm as u64)); },

        // --- System ---
        Instruction::Fence  => { /* NOP in Phase 1 — no memory-ordering concerns */ },
        Instruction::Ecall  => return Err(anyhow!("ecall at pc={pc:#x}")),
        Instruction::Ebreak => return Err(anyhow!("ebreak at pc={pc:#x}")),
    }

    cpu.pc = next_pc;
    Ok(())
}

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
}
