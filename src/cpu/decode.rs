use anyhow::{anyhow, Result};

/// All RV64I instructions. Fields are pre-decoded and sign-extended.
/// Immediates are i64 (signed). Shift amounts are u32 (6-bit for 64-bit ops,
/// 5-bit for *W ops). Register indices are usize.
#[derive(Debug, PartialEq)]
pub enum Instruction {
    // --- R-type (opcode 0x33) ---
    Add  { rd: usize, rs1: usize, rs2: usize },
    Sub  { rd: usize, rs1: usize, rs2: usize },
    Sll  { rd: usize, rs1: usize, rs2: usize },
    Slt  { rd: usize, rs1: usize, rs2: usize },
    Sltu { rd: usize, rs1: usize, rs2: usize },
    Xor  { rd: usize, rs1: usize, rs2: usize },
    Srl  { rd: usize, rs1: usize, rs2: usize },
    Sra  { rd: usize, rs1: usize, rs2: usize },
    Or   { rd: usize, rs1: usize, rs2: usize },
    And  { rd: usize, rs1: usize, rs2: usize },
    // --- M extension (opcode 0x33 / 0x3B, funct7=0x01) ---
    // Spec: Unprivileged §7
    Mul    { rd: usize, rs1: usize, rs2: usize },
    Mulh   { rd: usize, rs1: usize, rs2: usize },
    Mulhsu { rd: usize, rs1: usize, rs2: usize },
    Mulhu  { rd: usize, rs1: usize, rs2: usize },
    Div    { rd: usize, rs1: usize, rs2: usize },
    Divu   { rd: usize, rs1: usize, rs2: usize },
    Rem    { rd: usize, rs1: usize, rs2: usize },
    Remu   { rd: usize, rs1: usize, rs2: usize },
    // W-variants (opcode 0x3B, funct7=0x01)
    Mulw   { rd: usize, rs1: usize, rs2: usize },
    Divw   { rd: usize, rs1: usize, rs2: usize },
    Divuw  { rd: usize, rs1: usize, rs2: usize },
    Remw   { rd: usize, rs1: usize, rs2: usize },
    Remuw  { rd: usize, rs1: usize, rs2: usize },
    // --- RV64I W-variants R-type (opcode 0x3B) ---
    Addw { rd: usize, rs1: usize, rs2: usize },
    Subw { rd: usize, rs1: usize, rs2: usize },
    Sllw { rd: usize, rs1: usize, rs2: usize },
    Srlw { rd: usize, rs1: usize, rs2: usize },
    Sraw { rd: usize, rs1: usize, rs2: usize },
    // --- I-type arithmetic (opcode 0x13) ---
    Addi  { rd: usize, rs1: usize, imm: i64 },
    Slti  { rd: usize, rs1: usize, imm: i64 },
    Sltiu { rd: usize, rs1: usize, imm: i64 },
    Xori  { rd: usize, rs1: usize, imm: i64 },
    Ori   { rd: usize, rs1: usize, imm: i64 },
    Andi  { rd: usize, rs1: usize, imm: i64 },
    Slli  { rd: usize, rs1: usize, shamt: u32 },  // 6-bit shamt
    Srli  { rd: usize, rs1: usize, shamt: u32 },
    Srai  { rd: usize, rs1: usize, shamt: u32 },
    // --- RV64I IW-variants (opcode 0x1B) ---
    Addiw { rd: usize, rs1: usize, imm: i64 },
    Slliw { rd: usize, rs1: usize, shamt: u32 },  // 5-bit shamt
    Srliw { rd: usize, rs1: usize, shamt: u32 },
    Sraiw { rd: usize, rs1: usize, shamt: u32 },
    // --- Loads (opcode 0x03) ---
    Lb  { rd: usize, rs1: usize, imm: i64 },
    Lh  { rd: usize, rs1: usize, imm: i64 },
    Lw  { rd: usize, rs1: usize, imm: i64 },
    Ld  { rd: usize, rs1: usize, imm: i64 },
    Lbu { rd: usize, rs1: usize, imm: i64 },
    Lhu { rd: usize, rs1: usize, imm: i64 },
    Lwu { rd: usize, rs1: usize, imm: i64 },
    // --- Stores (opcode 0x23) ---
    Sb { rs1: usize, rs2: usize, imm: i64 },
    Sh { rs1: usize, rs2: usize, imm: i64 },
    Sw { rs1: usize, rs2: usize, imm: i64 },
    Sd { rs1: usize, rs2: usize, imm: i64 },
    // --- Branches (opcode 0x63) ---
    Beq  { rs1: usize, rs2: usize, imm: i64 },
    Bne  { rs1: usize, rs2: usize, imm: i64 },
    Blt  { rs1: usize, rs2: usize, imm: i64 },
    Bge  { rs1: usize, rs2: usize, imm: i64 },
    Bltu { rs1: usize, rs2: usize, imm: i64 },
    Bgeu { rs1: usize, rs2: usize, imm: i64 },
    // --- Jumps ---
    Jal  { rd: usize, imm: i64 },              // opcode 0x6F
    Jalr { rd: usize, rs1: usize, imm: i64 },  // opcode 0x67
    // --- Upper immediate ---
    Lui   { rd: usize, imm: i64 },  // opcode 0x37
    Auipc { rd: usize, imm: i64 },  // opcode 0x17
    // --- System ---
    Fence,   // opcode 0x0F — NOP in Phase 1
    Ecall,   // opcode 0x73, imm=0
    Ebreak,  // opcode 0x73, imm=1
    Mret,    // opcode 0x73, imm=0x302 — return from M-mode trap
    Sret,    // opcode 0x73, imm=0x102 — return from S-mode trap (Phase 3)
    Wfi,     // opcode 0x73, imm=0x105 — wait for interrupt (NOP for now)
    // --- Zicsr extension (RV CSR access) ---
    // Spec: Unprivileged §9. csr is the 12-bit CSR address (bits[31:20]).
    // The *I variants use a 5-bit unsigned immediate from rs1 field (zero-extended).
    Csrrw  { rd: usize, rs1: usize, csr: u16 },
    Csrrs  { rd: usize, rs1: usize, csr: u16 },
    Csrrc  { rd: usize, rs1: usize, csr: u16 },
    Csrrwi { rd: usize, uimm: u32, csr: u16 },
    Csrrsi { rd: usize, uimm: u32, csr: u16 },
    Csrrci { rd: usize, uimm: u32, csr: u16 },
}

/// Sign-extend the high 12 bits of inst (I-type immediate).
/// Spec: Unprivileged §2.3
fn i_imm(inst: u32) -> i64 { ((inst as i32) >> 20) as i64 }

/// S-type immediate: inst[31:25] | inst[11:7], sign-extended.
fn s_imm(inst: u32) -> i64 {
    let raw = ((inst >> 25) << 5) | ((inst >> 7) & 0x1f);
    (((raw << 20) as i32) >> 20) as i64
}

/// B-type immediate: inst[31]|inst[7]|inst[30:25]|inst[11:8], shifted left 1.
fn b_imm(inst: u32) -> i64 {
    let raw = ((inst >> 31) << 12)
            | (((inst >> 7) & 0x1) << 11)
            | (((inst >> 25) & 0x3f) << 5)
            | (((inst >> 8) & 0xf) << 1);
    (((raw << 19) as i32) >> 19) as i64
}

/// U-type immediate: inst[31:12] << 12, sign-extended from bit 31.
fn u_imm(inst: u32) -> i64 { ((inst & 0xFFFF_F000) as i32) as i64 }

/// J-type immediate: inst[31]|inst[19:12]|inst[20]|inst[30:21], shifted left 1.
fn j_imm(inst: u32) -> i64 {
    let raw = ((inst >> 31) << 20)
            | ((inst & 0x000F_F000))
            | (((inst >> 20) & 0x1) << 11)
            | (((inst >> 21) & 0x3ff) << 1);
    (((raw << 11) as i32) >> 11) as i64
}

/// Extract register index from bit position lo.
fn reg(inst: u32, lo: u32) -> usize { ((inst >> lo) & 0x1f) as usize }

pub fn decode(inst: u32) -> Result<Instruction> {
    let opcode = inst & 0x7f;
    let funct3 = (inst >> 12) & 0x7;
    let funct7 = (inst >> 25) & 0x7f;
    let rd  = reg(inst, 7);
    let rs1 = reg(inst, 15);
    let rs2 = reg(inst, 20);

    match opcode {
        // R-type: opcode 0x33
        0x33 => match (funct3, funct7) {
            (0x0, 0x00) => Ok(Instruction::Add  { rd, rs1, rs2 }),
            (0x0, 0x20) => Ok(Instruction::Sub  { rd, rs1, rs2 }),
            (0x1, 0x00) => Ok(Instruction::Sll  { rd, rs1, rs2 }),
            (0x2, 0x00) => Ok(Instruction::Slt  { rd, rs1, rs2 }),
            (0x3, 0x00) => Ok(Instruction::Sltu { rd, rs1, rs2 }),
            (0x4, 0x00) => Ok(Instruction::Xor  { rd, rs1, rs2 }),
            (0x5, 0x00) => Ok(Instruction::Srl  { rd, rs1, rs2 }),
            (0x5, 0x20) => Ok(Instruction::Sra  { rd, rs1, rs2 }),
            (0x6, 0x00) => Ok(Instruction::Or   { rd, rs1, rs2 }),
            (0x7, 0x00) => Ok(Instruction::And  { rd, rs1, rs2 }),
            // M extension (funct7=0x01)
            (0x0, 0x01) => Ok(Instruction::Mul    { rd, rs1, rs2 }),
            (0x1, 0x01) => Ok(Instruction::Mulh   { rd, rs1, rs2 }),
            (0x2, 0x01) => Ok(Instruction::Mulhsu { rd, rs1, rs2 }),
            (0x3, 0x01) => Ok(Instruction::Mulhu  { rd, rs1, rs2 }),
            (0x4, 0x01) => Ok(Instruction::Div    { rd, rs1, rs2 }),
            (0x5, 0x01) => Ok(Instruction::Divu   { rd, rs1, rs2 }),
            (0x6, 0x01) => Ok(Instruction::Rem    { rd, rs1, rs2 }),
            (0x7, 0x01) => Ok(Instruction::Remu   { rd, rs1, rs2 }),
            _ => Err(anyhow!("illegal R-type funct3={funct3:#x} funct7={funct7:#x}")),
        },
        // RV64I W-variants R-type: opcode 0x3B
        0x3B => match (funct3, funct7) {
            (0x0, 0x00) => Ok(Instruction::Addw { rd, rs1, rs2 }),
            (0x0, 0x20) => Ok(Instruction::Subw { rd, rs1, rs2 }),
            (0x1, 0x00) => Ok(Instruction::Sllw { rd, rs1, rs2 }),
            (0x5, 0x00) => Ok(Instruction::Srlw { rd, rs1, rs2 }),
            (0x5, 0x20) => Ok(Instruction::Sraw { rd, rs1, rs2 }),
            // M extension W-variants (funct7=0x01)
            (0x0, 0x01) => Ok(Instruction::Mulw  { rd, rs1, rs2 }),
            (0x4, 0x01) => Ok(Instruction::Divw  { rd, rs1, rs2 }),
            (0x5, 0x01) => Ok(Instruction::Divuw { rd, rs1, rs2 }),
            (0x6, 0x01) => Ok(Instruction::Remw  { rd, rs1, rs2 }),
            (0x7, 0x01) => Ok(Instruction::Remuw { rd, rs1, rs2 }),
            _ => Err(anyhow!("illegal W R-type funct3={funct3:#x} funct7={funct7:#x}")),
        },
        // I-type arithmetic: opcode 0x13
        0x13 => match funct3 {
            0x0 => Ok(Instruction::Addi  { rd, rs1, imm: i_imm(inst) }),
            0x2 => Ok(Instruction::Slti  { rd, rs1, imm: i_imm(inst) }),
            0x3 => Ok(Instruction::Sltiu { rd, rs1, imm: i_imm(inst) }),
            0x4 => Ok(Instruction::Xori  { rd, rs1, imm: i_imm(inst) }),
            0x6 => Ok(Instruction::Ori   { rd, rs1, imm: i_imm(inst) }),
            0x7 => Ok(Instruction::Andi  { rd, rs1, imm: i_imm(inst) }),
            // Shifts: shamt is imm[5:0] for RV64I (6-bit)
            0x1 => Ok(Instruction::Slli  { rd, rs1, shamt: (inst >> 20) & 0x3f }),
            0x5 => {
                let shamt = (inst >> 20) & 0x3f;
                if (inst >> 30) & 1 == 0 {
                    Ok(Instruction::Srli { rd, rs1, shamt })
                } else {
                    Ok(Instruction::Srai { rd, rs1, shamt })
                }
            },
            _ => unreachable!(),
        },
        // RV64I IW-variants: opcode 0x1B
        0x1B => match funct3 {
            0x0 => Ok(Instruction::Addiw { rd, rs1, imm: i_imm(inst) }),
            // shamt is imm[4:0] for 32-bit ops (5-bit)
            0x1 => Ok(Instruction::Slliw { rd, rs1, shamt: (inst >> 20) & 0x1f }),
            0x5 => {
                let shamt = (inst >> 20) & 0x1f;
                if (inst >> 30) & 1 == 0 {
                    Ok(Instruction::Srliw { rd, rs1, shamt })
                } else {
                    Ok(Instruction::Sraiw { rd, rs1, shamt })
                }
            },
            _ => Err(anyhow!("illegal IW funct3={funct3:#x}")),
        },
        // Loads: opcode 0x03
        0x03 => {
            let imm = i_imm(inst);
            match funct3 {
                0x0 => Ok(Instruction::Lb  { rd, rs1, imm }),
                0x1 => Ok(Instruction::Lh  { rd, rs1, imm }),
                0x2 => Ok(Instruction::Lw  { rd, rs1, imm }),
                0x3 => Ok(Instruction::Ld  { rd, rs1, imm }),
                0x4 => Ok(Instruction::Lbu { rd, rs1, imm }),
                0x5 => Ok(Instruction::Lhu { rd, rs1, imm }),
                0x6 => Ok(Instruction::Lwu { rd, rs1, imm }),
                _ => Err(anyhow!("illegal load funct3={funct3:#x}")),
            }
        },
        // Stores: opcode 0x23
        0x23 => {
            let imm = s_imm(inst);
            match funct3 {
                0x0 => Ok(Instruction::Sb { rs1, rs2, imm }),
                0x1 => Ok(Instruction::Sh { rs1, rs2, imm }),
                0x2 => Ok(Instruction::Sw { rs1, rs2, imm }),
                0x3 => Ok(Instruction::Sd { rs1, rs2, imm }),
                _ => Err(anyhow!("illegal store funct3={funct3:#x}")),
            }
        },
        // Branches: opcode 0x63
        0x63 => {
            let imm = b_imm(inst);
            match funct3 {
                0x0 => Ok(Instruction::Beq  { rs1, rs2, imm }),
                0x1 => Ok(Instruction::Bne  { rs1, rs2, imm }),
                0x4 => Ok(Instruction::Blt  { rs1, rs2, imm }),
                0x5 => Ok(Instruction::Bge  { rs1, rs2, imm }),
                0x6 => Ok(Instruction::Bltu { rs1, rs2, imm }),
                0x7 => Ok(Instruction::Bgeu { rs1, rs2, imm }),
                _ => Err(anyhow!("illegal branch funct3={funct3:#x}")),
            }
        },
        0x6F => Ok(Instruction::Jal  { rd, imm: j_imm(inst) }),
        0x67 => Ok(Instruction::Jalr { rd, rs1, imm: i_imm(inst) }),
        0x37 => Ok(Instruction::Lui   { rd, imm: u_imm(inst) }),
        0x17 => Ok(Instruction::Auipc { rd, imm: u_imm(inst) }),
        0x0F => Ok(Instruction::Fence),
        // System / Zicsr: opcode 0x73. Spec: Unprivileged §9 (CSRs), Privileged §3.3 (mret).
        0x73 => {
            let csr = ((inst >> 20) & 0xfff) as u16;
            match funct3 {
                // funct3=0: PRIV instructions (ecall/ebreak/mret/sret/wfi). Distinguished by full imm.
                0x0 => match (csr, rs1, rd) {
                    (0x000, 0, 0) => Ok(Instruction::Ecall),
                    (0x001, 0, 0) => Ok(Instruction::Ebreak),
                    (0x302, 0, 0) => Ok(Instruction::Mret),
                    (0x102, 0, 0) => Ok(Instruction::Sret),
                    (0x105, 0, 0) => Ok(Instruction::Wfi),
                    _ => Err(anyhow!(
                        "illegal system instruction csr={csr:#x} rs1={rs1} rd={rd} inst={inst:#010x}"
                    )),
                },
                0x1 => Ok(Instruction::Csrrw  { rd, rs1, csr }),
                0x2 => Ok(Instruction::Csrrs  { rd, rs1, csr }),
                0x3 => Ok(Instruction::Csrrc  { rd, rs1, csr }),
                0x5 => Ok(Instruction::Csrrwi { rd, uimm: rs1 as u32, csr }),
                0x6 => Ok(Instruction::Csrrsi { rd, uimm: rs1 as u32, csr }),
                0x7 => Ok(Instruction::Csrrci { rd, uimm: rs1 as u32, csr }),
                _ => Err(anyhow!("illegal system funct3={funct3:#x} inst={inst:#010x}")),
            }
        },
        _ => Err(anyhow!("illegal opcode {opcode:#x} at inst={inst:#010x}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ADDI x1, x0, 42   →  0x02a00093
    #[test] fn decode_addi() {
        let inst = decode(0x02a00093).unwrap();
        assert_eq!(inst, Instruction::Addi { rd: 1, rs1: 0, imm: 42 });
    }

    // ADD x2, x1, x2   →  0x00208133
    // 0x00208133 = opcode=0110011 rd=00010(2) funct3=000 rs1=00001(1) rs2=00010(2) funct7=0000000
    #[test] fn decode_add() {
        let inst = decode(0x00208133).unwrap();
        assert_eq!(inst, Instruction::Add { rd: 2, rs1: 1, rs2: 2 });
    }

    // LW x5, 8(x2)   →  0x00812283
    #[test] fn decode_lw() {
        let inst = decode(0x00812283).unwrap();
        assert_eq!(inst, Instruction::Lw { rd: 5, rs1: 2, imm: 8 });
    }

    // SW x5, -4(x2)   →  0xfe512e23
    #[test] fn decode_sw() {
        let inst = decode(0xfe512e23).unwrap();
        assert_eq!(inst, Instruction::Sw { rs1: 2, rs2: 5, imm: -4 });
    }

    // BEQ x1, x2, +8   →  0x00208463
    #[test] fn decode_beq() {
        let inst = decode(0x00208463).unwrap();
        assert_eq!(inst, Instruction::Beq { rs1: 1, rs2: 2, imm: 8 });
    }

    // JAL x1, +4   →  0x004000ef
    #[test] fn decode_jal() {
        let inst = decode(0x004000ef).unwrap();
        assert_eq!(inst, Instruction::Jal { rd: 1, imm: 4 });
    }

    // LUI x1, imm=0x12345000
    // inst = 0x123450b7: opcode=0110111 rd=00001(1) imm[31:12]=0x12345
    #[test] fn decode_lui() {
        let inst = decode(0x123450b7).unwrap();
        assert_eq!(inst, Instruction::Lui { rd: 1, imm: 0x12345000 });
    }

    // SRAI x1, x1, 3   →  0x4030d093
    #[test] fn decode_srai() {
        let inst = decode(0x4030d093).unwrap();
        assert_eq!(inst, Instruction::Srai { rd: 1, rs1: 1, shamt: 3 });
    }

    // ADDIW x1, x1, -1   →  0xfff0809b
    #[test] fn decode_addiw() {
        let inst = decode(0xfff0809b).unwrap();
        assert_eq!(inst, Instruction::Addiw { rd: 1, rs1: 1, imm: -1 });
    }

    // MUL x3, x1, x2  →  0x0220_81B3
    #[test] fn decode_mul() {
        let inst = decode(0x022081B3).unwrap();
        assert_eq!(inst, Instruction::Mul { rd: 3, rs1: 1, rs2: 2 });
    }

    // MULW x3, x1, x2  →  0x0220_81BB
    #[test] fn decode_mulw() {
        let inst = decode(0x022081BB).unwrap();
        assert_eq!(inst, Instruction::Mulw { rd: 3, rs1: 1, rs2: 2 });
    }

    // DIV x3, x1, x2  →  0x0220_C1B3
    #[test] fn decode_div() {
        let inst = decode(0x0220C1B3).unwrap();
        assert_eq!(inst, Instruction::Div { rd: 3, rs1: 1, rs2: 2 });
    }
}
