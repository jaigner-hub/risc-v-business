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
}

pub fn decode(_inst: u32) -> Result<Instruction> {
    Err(anyhow!("decode not yet implemented"))
}
