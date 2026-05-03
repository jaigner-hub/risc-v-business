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

// NaN-boxing: single-precision values in 64-bit FP registers have upper 32 bits = 0xFFFFFFFF.
// If upper 32 bits are not all-1, the register holds an invalid value → read as canonical NaN.
const CANONICAL_NAN_S: u64 = 0xFFFF_FFFF_7FC0_0000; // NaN-boxed canonical quiet NaN
const CANONICAL_NAN_D: u64 = 0x7FF8_0000_0000_0000;

#[inline(always)] fn freg_f32(v: u64) -> f32 {
    if v >> 32 != 0xFFFF_FFFF { f32::from_bits(0x7FC0_0000) }
    else { f32::from_bits(v as u32) }
}
#[inline(always)] fn f32_to_freg(v: f32) -> u64 { 0xFFFF_FFFF_0000_0000 | v.to_bits() as u64 }
#[inline(always)] fn freg_f64(v: u64) -> f64  { f64::from_bits(v) }
#[inline(always)] fn f64_to_freg(v: f64) -> u64 { v.to_bits() }

fn fclass_s(v: f32) -> u64 {
    let bits = v.to_bits();
    let sign = bits >> 31;
    let exp  = (bits >> 23) & 0xFF;
    let mant = bits & 0x7F_FFFF;
    if exp == 0xFF && mant != 0 {
        if mant & 0x40_0000 != 0 { 1 << 9 } else { 1 << 8 } // qNaN / sNaN
    } else if exp == 0xFF { if sign != 0 { 1 << 0 } else { 1 << 7 } } // ±inf
    else if exp == 0 && mant == 0 { if sign != 0 { 1 << 3 } else { 1 << 4 } } // ±zero
    else if exp == 0 { if sign != 0 { 1 << 2 } else { 1 << 5 } } // ±subnormal
    else if sign != 0 { 1 << 1 } else { 1 << 6 } // ±normal
}

fn fclass_d(v: f64) -> u64 {
    let bits = v.to_bits();
    let sign = bits >> 63;
    let exp  = (bits >> 52) & 0x7FF;
    let mant = bits & 0x000F_FFFF_FFFF_FFFF;
    if exp == 0x7FF && mant != 0 {
        if mant & 0x0008_0000_0000_0000 != 0 { 1 << 9 } else { 1 << 8 }
    } else if exp == 0x7FF { if sign != 0 { 1 << 0 } else { 1 << 7 } }
    else if exp == 0 && mant == 0 { if sign != 0 { 1 << 3 } else { 1 << 4 } }
    else if exp == 0 { if sign != 0 { 1 << 2 } else { 1 << 5 } }
    else if sign != 0 { 1 << 1 } else { 1 << 6 }
}

// RISC-V min/max: if one operand is NaN, return the other; if both NaN, return canonical qNaN.
#[inline(always)] fn fp_min_s(a: f32, b: f32) -> f32 {
    if a.is_nan() { b } else if b.is_nan() { a } else if a <= b { a } else { b }
}
#[inline(always)] fn fp_max_s(a: f32, b: f32) -> f32 {
    if a.is_nan() { b } else if b.is_nan() { a } else if a >= b { a } else { b }
}
#[inline(always)] fn fp_min_d(a: f64, b: f64) -> f64 {
    if a.is_nan() { b } else if b.is_nan() { a } else if a <= b { a } else { b }
}
#[inline(always)] fn fp_max_d(a: f64, b: f64) -> f64 {
    if a.is_nan() { b } else if b.is_nan() { a } else if a >= b { a } else { b }
}

// Float→int saturating conversions per RISC-V spec §11.6 / §12.4.
// Out-of-range or NaN → saturate to type boundary.
#[inline(always)] fn fcvt_w_s(v: f32)  -> u64 {
    if v.is_nan() || v >= 2147483648.0f32  { 0x7FFF_FFFFu64 }
    else if v < -2147483648.0f32 { 0xFFFF_FFFF_8000_0000u64 }
    else { (v as i32) as i64 as u64 }
}
#[inline(always)] fn fcvt_wu_s(v: f32) -> u64 {
    if v.is_nan() || v >= 4294967296.0f32 { 0xFFFF_FFFFu64 }
    else if v < 0.0 { 0u64 } else { (v as u32) as u64 }
}
#[inline(always)] fn fcvt_l_s(v: f32) -> u64 {
    if v.is_nan() || v >= 9.223372036854776e18f32  { i64::MAX as u64 }
    else if v < -9.223372036854776e18f32 { i64::MIN as u64 }
    else { (v as i64) as u64 }
}
#[inline(always)] fn fcvt_lu_s(v: f32) -> u64 {
    if v.is_nan() || v >= 1.8446744073709552e19f32 { u64::MAX }
    else if v < 0.0 { 0u64 } else { v as u64 }
}
#[inline(always)] fn fcvt_w_d(v: f64) -> u64 {
    if v.is_nan() || v >= 2147483648.0f64  { 0x7FFF_FFFFu64 }
    else if v < -2147483648.0f64 { 0xFFFF_FFFF_8000_0000u64 }
    else { (v as i32) as i64 as u64 }
}
#[inline(always)] fn fcvt_wu_d(v: f64) -> u64 {
    if v.is_nan() || v >= 4294967296.0f64 { 0xFFFF_FFFFu64 }
    else if v < 0.0 { 0u64 } else { (v as u32) as u64 }
}
#[inline(always)] fn fcvt_l_d(v: f64) -> u64 {
    if v.is_nan() || v >= 9.223372036854776e18f64  { i64::MAX as u64 }
    else if v < -9.223372036854776e18f64 { i64::MIN as u64 }
    else { (v as i64) as u64 }
}
#[inline(always)] fn fcvt_lu_d(v: f64) -> u64 {
    if v.is_nan() || v >= 1.8446744073709552e19f64 { u64::MAX }
    else if v < 0.0 { 0u64 } else { v as u64 }
}

pub fn execute(cpu: &mut Cpu, inst: Instruction) -> Result<()> {
    let pc = cpu.pc;

    // Most instructions advance pc by inst_size (4 for full, 2 for RVC).
    let mut next_pc = pc.wrapping_add(cpu.inst_size);

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

        // --- FP loads/stores (F/D extension, Phase 5 minimal) ---
        // Spec: Unprivileged §11.5 (FLW/FSW), §12.3 (FLD/FSD).
        // No FP arithmetic — these are pure memory<->fregs movement so kernel
        // context save/restore (signal handlers, task switches) works.
        Instruction::Flw { fd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 4) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.fregs[fd] = 0xFFFF_FFFF_0000_0000u64 | (v & 0xFFFF_FFFF),
                }
            }
        },
        Instruction::Fld { fd, rs1, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Load) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.load(pa, 8) {
                    Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
                    Ok(v)  => cpu.fregs[fd] = v,
                }
            }
        },
        Instruction::Fsw { rs1, fs2, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.store(pa, 4, cpu.fregs[fs2]) {
                    Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
                    Ok(_)  => {}
                }
            }
        },
        Instruction::Fsd { rs1, fs2, imm } => {
            let va = cpu.reg(rs1).wrapping_add(imm as u64);
            match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, ls_mode, cpu.csr.mstatus, va, AccessType::Store) {
                Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
                Ok(pa) => match cpu.bus.store(pa, 8, cpu.fregs[fs2]) {
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
                if t & 0x1 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bne  { rs1, rs2, imm } => {
            if cpu.reg(rs1) != cpu.reg(rs2) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x1 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Blt  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) < (cpu.reg(rs2) as i64) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x1 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bge  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) >= (cpu.reg(rs2) as i64) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x1 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bltu { rs1, rs2, imm } => {
            if cpu.reg(rs1) < cpu.reg(rs2) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x1 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },
        Instruction::Bgeu { rs1, rs2, imm } => {
            if cpu.reg(rs1) >= cpu.reg(rs2) {
                let t = pc.wrapping_add(imm as u64);
                if t & 0x1 != 0 { cpu.deliver_trap(0, t); next_pc = cpu.pc; }
                else { next_pc = t; }
            }
        },

        // --- Jumps ---
        // JAL: rd = pc+inst_size, pc = pc + imm. Spec: Unprivileged §2.5
        Instruction::Jal { rd, imm } => {
            let target = pc.wrapping_add(imm as u64);
            if target & 0x1 != 0 {
                cpu.deliver_trap(0, target); next_pc = cpu.pc;
            } else {
                cpu.set_reg(rd, next_pc);
                next_pc = target;
            }
        },
        // JALR: rd = pc+inst_size, pc = (rs1 + imm) & ~1. Spec: Unprivileged §2.5
        Instruction::Jalr { rd, rs1, imm } => {
            let target = cpu.reg(rs1).wrapping_add(imm as u64) & !1;
            if target & 0x1 != 0 {
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
                // Flush the SW TLB — VA→PA translations may have changed.
                // Do NOT set jit_invalidate here: sfence.vma in practice fires
                // for set_memory_rw/ro (permission-only PTE changes) as well as
                // context-switch shootdowns. In both cases the VA→PA mapping is
                // unchanged so compiled JIT blocks remain valid. Real address-space
                // switches always write satp, which already sets jit_invalidate.
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
                if csr == 0x180 {
                    cpu.mmu.flush();
                    cpu.jit_invalidate = true;
                }
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
                if csr == 0x180 && rs1 != 0 {
                    cpu.mmu.flush();
                    cpu.jit_invalidate = true;
                }
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
                if csr == 0x180 && rs1 != 0 {
                    cpu.mmu.flush();
                    cpu.jit_invalidate = true;
                }
            }
        },
        Instruction::Csrrwi { rd, uimm, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = if rd != 0 { cpu.csr_read(csr) } else { 0 };
                cpu.csr_write(csr, uimm as u64);
                cpu.set_reg(rd, old);
                if csr == 0x180 {
                    cpu.mmu.flush();
                    cpu.jit_invalidate = true;
                }
            }
        },
        Instruction::Csrrsi { rd, uimm, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = cpu.csr_read(csr);
                if uimm != 0 { cpu.csr_write(csr, old | uimm as u64); }
                cpu.set_reg(rd, old);
                if csr == 0x180 && uimm != 0 {
                    cpu.mmu.flush();
                    cpu.jit_invalidate = true;
                }
            }
        },
        Instruction::Csrrci { rd, uimm, csr } => {
            if csr == 0x180 && cpu.mode == PrivMode::S && (cpu.csr.mstatus >> 20) & 1 != 0 {
                cpu.deliver_trap(2, 0); next_pc = cpu.pc;
            } else {
                let old = cpu.csr_read(csr);
                if uimm != 0 { cpu.csr_write(csr, old & !(uimm as u64)); }
                cpu.set_reg(rd, old);
                if csr == 0x180 && uimm != 0 {
                    cpu.mmu.flush();
                    cpu.jit_invalidate = true;
                }
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

        // --- F/D extension arithmetic (OP-FP: opcode 0x53) ---
        // Spec: Unprivileged §11.6 (F), §12.4 (D).
        // Bit moves (no NaN-boxing check on read, no conversion)
        Instruction::FmvXW { rd, rs1 }  => { cpu.set_reg(rd, sext(cpu.fregs[rs1] & 0xFFFF_FFFF, 31)); },
        Instruction::FmvXD { rd, rs1 }  => { cpu.set_reg(rd, cpu.fregs[rs1]); },
        Instruction::FmvWX { rd, rs1 }  => { cpu.fregs[rd] = f32_to_freg(f32::from_bits(cpu.reg(rs1) as u32)); },
        Instruction::FmvDX { rd, rs1 }  => { cpu.fregs[rd] = cpu.reg(rs1); },
        Instruction::FclassS { rd, rs1 } => { cpu.set_reg(rd, fclass_s(freg_f32(cpu.fregs[rs1]))); },
        Instruction::FclassD { rd, rs1 } => { cpu.set_reg(rd, fclass_d(freg_f64(cpu.fregs[rs1]))); },

        // Sign injection
        Instruction::FsgnjS  { rd, rs1, rs2 } => {
            let v = (freg_f32(cpu.fregs[rs1]).to_bits() & 0x7FFF_FFFF) | (freg_f32(cpu.fregs[rs2]).to_bits() & 0x8000_0000);
            cpu.fregs[rd] = f32_to_freg(f32::from_bits(v));
        },
        Instruction::FsgnjnS { rd, rs1, rs2 } => {
            let v = (freg_f32(cpu.fregs[rs1]).to_bits() & 0x7FFF_FFFF) | (!freg_f32(cpu.fregs[rs2]).to_bits() & 0x8000_0000);
            cpu.fregs[rd] = f32_to_freg(f32::from_bits(v));
        },
        Instruction::FsgnjxS { rd, rs1, rs2 } => {
            let v = freg_f32(cpu.fregs[rs1]).to_bits() ^ (freg_f32(cpu.fregs[rs2]).to_bits() & 0x8000_0000);
            cpu.fregs[rd] = f32_to_freg(f32::from_bits(v));
        },
        Instruction::FsgnjD  { rd, rs1, rs2 } => {
            let v = (cpu.fregs[rs1] & 0x7FFF_FFFF_FFFF_FFFF) | (cpu.fregs[rs2] & 0x8000_0000_0000_0000);
            cpu.fregs[rd] = v;
        },
        Instruction::FsgnjnD { rd, rs1, rs2 } => {
            let v = (cpu.fregs[rs1] & 0x7FFF_FFFF_FFFF_FFFF) | (!cpu.fregs[rs2] & 0x8000_0000_0000_0000);
            cpu.fregs[rd] = v;
        },
        Instruction::FsgnjxD { rd, rs1, rs2 } => {
            let v = cpu.fregs[rs1] ^ (cpu.fregs[rs2] & 0x8000_0000_0000_0000);
            cpu.fregs[rd] = v;
        },

        // Min/Max (RISC-V semantics: NaN-tolerant, return non-NaN operand)
        Instruction::FminS { rd, rs1, rs2 } => {
            let r = fp_min_s(freg_f32(cpu.fregs[rs1]), freg_f32(cpu.fregs[rs2]));
            cpu.fregs[rd] = if r.is_nan() { CANONICAL_NAN_S } else { f32_to_freg(r) };
        },
        Instruction::FmaxS { rd, rs1, rs2 } => {
            let r = fp_max_s(freg_f32(cpu.fregs[rs1]), freg_f32(cpu.fregs[rs2]));
            cpu.fregs[rd] = if r.is_nan() { CANONICAL_NAN_S } else { f32_to_freg(r) };
        },
        Instruction::FminD { rd, rs1, rs2 } => {
            let r = fp_min_d(freg_f64(cpu.fregs[rs1]), freg_f64(cpu.fregs[rs2]));
            cpu.fregs[rd] = if r.is_nan() { CANONICAL_NAN_D } else { f64_to_freg(r) };
        },
        Instruction::FmaxD { rd, rs1, rs2 } => {
            let r = fp_max_d(freg_f64(cpu.fregs[rs1]), freg_f64(cpu.fregs[rs2]));
            cpu.fregs[rd] = if r.is_nan() { CANONICAL_NAN_D } else { f64_to_freg(r) };
        },

        // Comparisons → integer result (false if either operand is NaN, except FEQ where NaN→0)
        Instruction::FleS { rd, rs1, rs2 } => {
            let (a, b) = (freg_f32(cpu.fregs[rs1]), freg_f32(cpu.fregs[rs2]));
            cpu.set_reg(rd, (!a.is_nan() && !b.is_nan() && a <= b) as u64);
        },
        Instruction::FltS { rd, rs1, rs2 } => {
            let (a, b) = (freg_f32(cpu.fregs[rs1]), freg_f32(cpu.fregs[rs2]));
            cpu.set_reg(rd, (!a.is_nan() && !b.is_nan() && a < b) as u64);
        },
        Instruction::FeqS { rd, rs1, rs2 } => {
            let (a, b) = (freg_f32(cpu.fregs[rs1]), freg_f32(cpu.fregs[rs2]));
            cpu.set_reg(rd, (!a.is_nan() && !b.is_nan() && a == b) as u64);
        },
        Instruction::FleD { rd, rs1, rs2 } => {
            let (a, b) = (freg_f64(cpu.fregs[rs1]), freg_f64(cpu.fregs[rs2]));
            cpu.set_reg(rd, (!a.is_nan() && !b.is_nan() && a <= b) as u64);
        },
        Instruction::FltD { rd, rs1, rs2 } => {
            let (a, b) = (freg_f64(cpu.fregs[rs1]), freg_f64(cpu.fregs[rs2]));
            cpu.set_reg(rd, (!a.is_nan() && !b.is_nan() && a < b) as u64);
        },
        Instruction::FeqD { rd, rs1, rs2 } => {
            let (a, b) = (freg_f64(cpu.fregs[rs1]), freg_f64(cpu.fregs[rs2]));
            cpu.set_reg(rd, (!a.is_nan() && !b.is_nan() && a == b) as u64);
        },

        // Arithmetic — rm field ignored (Rust uses IEEE 754 round-to-nearest-even by default)
        Instruction::FaddS  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f32_to_freg(freg_f32(cpu.fregs[rs1]) + freg_f32(cpu.fregs[rs2])); },
        Instruction::FsubS  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f32_to_freg(freg_f32(cpu.fregs[rs1]) - freg_f32(cpu.fregs[rs2])); },
        Instruction::FmulS  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f32_to_freg(freg_f32(cpu.fregs[rs1]) * freg_f32(cpu.fregs[rs2])); },
        Instruction::FdivS  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f32_to_freg(freg_f32(cpu.fregs[rs1]) / freg_f32(cpu.fregs[rs2])); },
        Instruction::FsqrtS { rd, rs1, ..      } => { cpu.fregs[rd] = f32_to_freg(freg_f32(cpu.fregs[rs1]).sqrt()); },
        Instruction::FaddD  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f64_to_freg(freg_f64(cpu.fregs[rs1]) + freg_f64(cpu.fregs[rs2])); },
        Instruction::FsubD  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f64_to_freg(freg_f64(cpu.fregs[rs1]) - freg_f64(cpu.fregs[rs2])); },
        Instruction::FmulD  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f64_to_freg(freg_f64(cpu.fregs[rs1]) * freg_f64(cpu.fregs[rs2])); },
        Instruction::FdivD  { rd, rs1, rs2, .. } => { cpu.fregs[rd] = f64_to_freg(freg_f64(cpu.fregs[rs1]) / freg_f64(cpu.fregs[rs2])); },
        Instruction::FsqrtD { rd, rs1, ..      } => { cpu.fregs[rd] = f64_to_freg(freg_f64(cpu.fregs[rs1]).sqrt()); },

        // Cross-precision conversions
        Instruction::FcvtSD { rd, rs1, .. } => { cpu.fregs[rd] = f32_to_freg(freg_f64(cpu.fregs[rs1]) as f32); },
        Instruction::FcvtDS { rd, rs1, .. } => { cpu.fregs[rd] = f64_to_freg(freg_f32(cpu.fregs[rs1]) as f64); },

        // Float→int (saturating out-of-range, sign-extend W results to 64 bits)
        Instruction::FcvtWS  { rd, rs1, .. } => { cpu.set_reg(rd, fcvt_w_s(freg_f32(cpu.fregs[rs1]))); },
        Instruction::FcvtWUS { rd, rs1, .. } => { cpu.set_reg(rd, sext(fcvt_wu_s(freg_f32(cpu.fregs[rs1])), 31)); },
        Instruction::FcvtLS  { rd, rs1, .. } => { cpu.set_reg(rd, fcvt_l_s(freg_f32(cpu.fregs[rs1]))); },
        Instruction::FcvtLUS { rd, rs1, .. } => { cpu.set_reg(rd, fcvt_lu_s(freg_f32(cpu.fregs[rs1]))); },
        Instruction::FcvtWD  { rd, rs1, .. } => { cpu.set_reg(rd, fcvt_w_d(freg_f64(cpu.fregs[rs1]))); },
        Instruction::FcvtWUD { rd, rs1, .. } => { cpu.set_reg(rd, sext(fcvt_wu_d(freg_f64(cpu.fregs[rs1])), 31)); },
        Instruction::FcvtLD  { rd, rs1, .. } => { cpu.set_reg(rd, fcvt_l_d(freg_f64(cpu.fregs[rs1]))); },
        Instruction::FcvtLUD { rd, rs1, .. } => { cpu.set_reg(rd, fcvt_lu_d(freg_f64(cpu.fregs[rs1]))); },

        // Int→float
        Instruction::FcvtSW  { rd, rs1, .. } => { cpu.fregs[rd] = f32_to_freg(cpu.reg(rs1) as i32 as f32); },
        Instruction::FcvtSWU { rd, rs1, .. } => { cpu.fregs[rd] = f32_to_freg(cpu.reg(rs1) as u32 as f32); },
        Instruction::FcvtSL  { rd, rs1, .. } => { cpu.fregs[rd] = f32_to_freg(cpu.reg(rs1) as i64 as f32); },
        Instruction::FcvtSLU { rd, rs1, .. } => { cpu.fregs[rd] = f32_to_freg(cpu.reg(rs1) as f32); },
        Instruction::FcvtDW  { rd, rs1, .. } => { cpu.fregs[rd] = f64_to_freg(cpu.reg(rs1) as i32 as f64); },
        Instruction::FcvtDWU { rd, rs1, .. } => { cpu.fregs[rd] = f64_to_freg(cpu.reg(rs1) as u32 as f64); },
        Instruction::FcvtDL  { rd, rs1, .. } => { cpu.fregs[rd] = f64_to_freg(cpu.reg(rs1) as i64 as f64); },
        Instruction::FcvtDLU { rd, rs1, .. } => { cpu.fregs[rd] = f64_to_freg(cpu.reg(rs1) as f64); },

        // Fused multiply-add (R4-type). Spec: §11.7 (F), §12.5 (D).
        // FMA: rd = rs1*rs2 + rs3
        // FMS: rd = rs1*rs2 - rs3
        // FNMS: rd = -(rs1*rs2) + rs3   (negate product, add addend)
        // FNMA: rd = -(rs1*rs2) - rs3   (negate product, subtract addend)
        Instruction::FmaddS  { rd, rs1, rs2, rs3, .. } => {
            let r = freg_f32(cpu.fregs[rs1]).mul_add(freg_f32(cpu.fregs[rs2]), freg_f32(cpu.fregs[rs3]));
            cpu.fregs[rd] = f32_to_freg(r);
        },
        Instruction::FmsubS  { rd, rs1, rs2, rs3, .. } => {
            let r = freg_f32(cpu.fregs[rs1]).mul_add(freg_f32(cpu.fregs[rs2]), -freg_f32(cpu.fregs[rs3]));
            cpu.fregs[rd] = f32_to_freg(r);
        },
        Instruction::FnmsubS { rd, rs1, rs2, rs3, .. } => {
            let r = (-freg_f32(cpu.fregs[rs1])).mul_add(freg_f32(cpu.fregs[rs2]), freg_f32(cpu.fregs[rs3]));
            cpu.fregs[rd] = f32_to_freg(r);
        },
        Instruction::FnmaddS { rd, rs1, rs2, rs3, .. } => {
            let r = (-freg_f32(cpu.fregs[rs1])).mul_add(freg_f32(cpu.fregs[rs2]), -freg_f32(cpu.fregs[rs3]));
            cpu.fregs[rd] = f32_to_freg(r);
        },
        Instruction::FmaddD  { rd, rs1, rs2, rs3, .. } => {
            let r = freg_f64(cpu.fregs[rs1]).mul_add(freg_f64(cpu.fregs[rs2]), freg_f64(cpu.fregs[rs3]));
            cpu.fregs[rd] = f64_to_freg(r);
        },
        Instruction::FmsubD  { rd, rs1, rs2, rs3, .. } => {
            let r = freg_f64(cpu.fregs[rs1]).mul_add(freg_f64(cpu.fregs[rs2]), -freg_f64(cpu.fregs[rs3]));
            cpu.fregs[rd] = f64_to_freg(r);
        },
        Instruction::FnmsubD { rd, rs1, rs2, rs3, .. } => {
            let r = (-freg_f64(cpu.fregs[rs1])).mul_add(freg_f64(cpu.fregs[rs2]), freg_f64(cpu.fregs[rs3]));
            cpu.fregs[rd] = f64_to_freg(r);
        },
        Instruction::FnmaddD { rd, rs1, rs2, rs3, .. } => {
            let r = (-freg_f64(cpu.fregs[rs1])).mul_add(freg_f64(cpu.fregs[rs2]), -freg_f64(cpu.fregs[rs3]));
            cpu.fregs[rd] = f64_to_freg(r);
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
