use anyhow::Result;

#[derive(Debug)]
pub struct IllegalInstruction(pub u32);

impl std::fmt::Display for IllegalInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal instruction: {:#010x}", self.0)
    }
}

impl std::error::Error for IllegalInstruction {}

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
    // --- A extension (opcode 0x2F) ---
    // Spec: Unprivileged §8. aq/rl bits decoded but ignored (single-hart).
    LrD  { rd: usize, rs1: usize },
    LrW  { rd: usize, rs1: usize },
    ScD  { rd: usize, rs1: usize, rs2: usize },
    ScW  { rd: usize, rs1: usize, rs2: usize },
    AmoswapD { rd: usize, rs1: usize, rs2: usize },
    AmoswapW { rd: usize, rs1: usize, rs2: usize },
    AmoaddD  { rd: usize, rs1: usize, rs2: usize },
    AmoaddW  { rd: usize, rs1: usize, rs2: usize },
    AmoxorD  { rd: usize, rs1: usize, rs2: usize },
    AmoxorW  { rd: usize, rs1: usize, rs2: usize },
    AmoandD  { rd: usize, rs1: usize, rs2: usize },
    AmoandW  { rd: usize, rs1: usize, rs2: usize },
    AmoorD   { rd: usize, rs1: usize, rs2: usize },
    AmoorW   { rd: usize, rs1: usize, rs2: usize },
    AmominD  { rd: usize, rs1: usize, rs2: usize },
    AmominW  { rd: usize, rs1: usize, rs2: usize },
    AmomaxD  { rd: usize, rs1: usize, rs2: usize },
    AmomaxW  { rd: usize, rs1: usize, rs2: usize },
    AmominuD { rd: usize, rs1: usize, rs2: usize },
    AmominuW { rd: usize, rs1: usize, rs2: usize },
    AmomaxuD { rd: usize, rs1: usize, rs2: usize },
    AmomaxuW { rd: usize, rs1: usize, rs2: usize },
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
    Sret,    // opcode 0x73, imm=0x102 — return from S-mode trap (TSR-conditional)
    Wfi,         // opcode 0x73, imm=0x105 — wait for interrupt (NOP for now)
    SfenceVma,   // opcode 0x73, funct7=0x09 — TLB shootdown (Priv §4.2.1)
    // --- Zicsr extension (RV CSR access) ---
    // Spec: Unprivileged §9. csr is the 12-bit CSR address (bits[31:20]).
    // The *I variants use a 5-bit unsigned immediate from rs1 field (zero-extended).
    Csrrw  { rd: usize, rs1: usize, csr: u16 },
    Csrrs  { rd: usize, rs1: usize, csr: u16 },
    Csrrc  { rd: usize, rs1: usize, csr: u16 },
    Csrrwi { rd: usize, uimm: u32, csr: u16 },
    Csrrsi { rd: usize, uimm: u32, csr: u16 },
    Csrrci { rd: usize, uimm: u32, csr: u16 },
    // --- F/D extension load/store ---
    // Spec: Unprivileged §11 (F), §12 (D).
    Flw { fd: usize, rs1: usize, imm: i64 },  // opcode 0x07, funct3=010
    Fld { fd: usize, rs1: usize, imm: i64 },  // opcode 0x07, funct3=011
    Fsw { rs1: usize, fs2: usize, imm: i64 }, // opcode 0x27, funct3=010
    Fsd { rs1: usize, fs2: usize, imm: i64 }, // opcode 0x27, funct3=011
    // --- F/D extension arithmetic (OP-FP: opcode 0x53) ---
    // Spec: Unprivileged §11.6 (F), §12.4 (D)
    // Bit moves between integer and FP register files
    FmvXW   { rd: usize, rs1: usize },           // FMV.X.W funct7=0x70 rs2=0
    FmvWX   { rd: usize, rs1: usize },           // FMV.W.X funct7=0x78 rs2=0
    FmvXD   { rd: usize, rs1: usize },           // FMV.X.D funct7=0x71 rs2=0
    FmvDX   { rd: usize, rs1: usize },           // FMV.D.X funct7=0x79 rs2=0
    FclassS { rd: usize, rs1: usize },           // FCLASS.S funct7=0x70 rs2=1
    FclassD { rd: usize, rs1: usize },           // FCLASS.D funct7=0x71 rs2=1
    // Sign injection: rm field encodes variant (FSGNJ=0, FSGNJN=1, FSGNJX=2)
    FsgnjS  { rd: usize, rs1: usize, rs2: usize },
    FsgnjnS { rd: usize, rs1: usize, rs2: usize },
    FsgnjxS { rd: usize, rs1: usize, rs2: usize },
    FsgnjD  { rd: usize, rs1: usize, rs2: usize },
    FsgnjnD { rd: usize, rs1: usize, rs2: usize },
    FsgnjxD { rd: usize, rs1: usize, rs2: usize },
    // Min/Max
    FminS   { rd: usize, rs1: usize, rs2: usize },
    FmaxS   { rd: usize, rs1: usize, rs2: usize },
    FminD   { rd: usize, rs1: usize, rs2: usize },
    FmaxD   { rd: usize, rs1: usize, rs2: usize },
    // Comparisons (result goes to integer rd)
    FleS    { rd: usize, rs1: usize, rs2: usize },
    FltS    { rd: usize, rs1: usize, rs2: usize },
    FeqS    { rd: usize, rs1: usize, rs2: usize },
    FleD    { rd: usize, rs1: usize, rs2: usize },
    FltD    { rd: usize, rs1: usize, rs2: usize },
    FeqD    { rd: usize, rs1: usize, rs2: usize },
    // Arithmetic (rm = rounding mode: 0=RNE 1=RTZ 2=RDN 3=RUP 4=RMM 7=DYN)
    FaddS   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FsubS   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FmulS   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FdivS   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FsqrtS  { rd: usize, rs1: usize, rm: u8 },
    FaddD   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FsubD   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FmulD   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FdivD   { rd: usize, rs1: usize, rs2: usize, rm: u8 },
    FsqrtD  { rd: usize, rs1: usize, rm: u8 },
    // Cross-precision conversions
    FcvtSD  { rd: usize, rs1: usize, rm: u8 },  // FCVT.S.D
    FcvtDS  { rd: usize, rs1: usize, rm: u8 },  // FCVT.D.S
    // Float-to-int conversions (rs2 field encodes target integer type)
    FcvtWS  { rd: usize, rs1: usize, rm: u8 },  // FCVT.W.S
    FcvtWUS { rd: usize, rs1: usize, rm: u8 },  // FCVT.WU.S
    FcvtLS  { rd: usize, rs1: usize, rm: u8 },  // FCVT.L.S  (RV64)
    FcvtLUS { rd: usize, rs1: usize, rm: u8 },  // FCVT.LU.S (RV64)
    FcvtWD  { rd: usize, rs1: usize, rm: u8 },  // FCVT.W.D
    FcvtWUD { rd: usize, rs1: usize, rm: u8 },  // FCVT.WU.D
    FcvtLD  { rd: usize, rs1: usize, rm: u8 },  // FCVT.L.D  (RV64)
    FcvtLUD { rd: usize, rs1: usize, rm: u8 },  // FCVT.LU.D (RV64)
    // Int-to-float conversions
    FcvtSW  { rd: usize, rs1: usize, rm: u8 },  // FCVT.S.W
    FcvtSWU { rd: usize, rs1: usize, rm: u8 },  // FCVT.S.WU
    FcvtSL  { rd: usize, rs1: usize, rm: u8 },  // FCVT.S.L  (RV64)
    FcvtSLU { rd: usize, rs1: usize, rm: u8 },  // FCVT.S.LU (RV64)
    FcvtDW  { rd: usize, rs1: usize, rm: u8 },  // FCVT.D.W
    FcvtDWU { rd: usize, rs1: usize, rm: u8 },  // FCVT.D.WU
    FcvtDL  { rd: usize, rs1: usize, rm: u8 },  // FCVT.D.L  (RV64)
    FcvtDLU { rd: usize, rs1: usize, rm: u8 },  // FCVT.D.LU (RV64)
    // Fused multiply-add (R4-type: opcodes 0x43/0x47/0x4B/0x4F, fmt bit[26:25])
    // Spec: Unprivileged §11.7 (F), §12.5 (D)
    FmaddS  { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
    FmsubS  { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
    FnmsubS { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
    FnmaddS { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
    FmaddD  { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
    FmsubD  { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
    FnmsubD { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
    FnmaddD { rd: usize, rs1: usize, rs2: usize, rs3: usize, rm: u8 },
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
            _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
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
            _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
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
            _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
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
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
            }
        },
        // FP loads (LOAD_FP): opcode 0x07 — I-type. Spec: Unprivileged §11.5, §12.3
        0x07 => {
            let imm = i_imm(inst);
            // rd field encodes the FP destination register (fd).
            match funct3 {
                0x2 => Ok(Instruction::Flw { fd: rd, rs1, imm }),
                0x3 => Ok(Instruction::Fld { fd: rd, rs1, imm }),
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
            }
        },
        // FP stores (STORE_FP): opcode 0x27 — S-type. Spec: Unprivileged §11.5, §12.3
        0x27 => {
            let imm = s_imm(inst);
            // rs2 field encodes the FP source register (fs2).
            match funct3 {
                0x2 => Ok(Instruction::Fsw { rs1, fs2: rs2, imm }),
                0x3 => Ok(Instruction::Fsd { rs1, fs2: rs2, imm }),
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
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
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
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
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
            }
        },
        0x6F => Ok(Instruction::Jal  { rd, imm: j_imm(inst) }),
        0x67 => Ok(Instruction::Jalr { rd, rs1, imm: i_imm(inst) }),
        0x37 => Ok(Instruction::Lui   { rd, imm: u_imm(inst) }),
        0x17 => Ok(Instruction::Auipc { rd, imm: u_imm(inst) }),
        // A extension: opcode 0x2F
        // funct5 = inst[31:27]; funct3 bit0: 0=word(W), 1=double(D); aq/rl ignored.
        // Spec: Unprivileged §8.2
        0x2F => {
            let funct5 = (inst >> 27) & 0x1f;
            let is_double = (funct3 & 0x1) == 1; // funct3=2(W) or 3(D)
            match (funct5, is_double) {
                (0x02, true)  => Ok(Instruction::LrD  { rd, rs1 }),
                (0x02, false) => Ok(Instruction::LrW  { rd, rs1 }),
                (0x03, true)  => Ok(Instruction::ScD  { rd, rs1, rs2 }),
                (0x03, false) => Ok(Instruction::ScW  { rd, rs1, rs2 }),
                (0x01, true)  => Ok(Instruction::AmoswapD { rd, rs1, rs2 }),
                (0x01, false) => Ok(Instruction::AmoswapW { rd, rs1, rs2 }),
                (0x00, true)  => Ok(Instruction::AmoaddD  { rd, rs1, rs2 }),
                (0x00, false) => Ok(Instruction::AmoaddW  { rd, rs1, rs2 }),
                (0x04, true)  => Ok(Instruction::AmoxorD  { rd, rs1, rs2 }),
                (0x04, false) => Ok(Instruction::AmoxorW  { rd, rs1, rs2 }),
                (0x0C, true)  => Ok(Instruction::AmoandD  { rd, rs1, rs2 }),
                (0x0C, false) => Ok(Instruction::AmoandW  { rd, rs1, rs2 }),
                (0x08, true)  => Ok(Instruction::AmoorD   { rd, rs1, rs2 }),
                (0x08, false) => Ok(Instruction::AmoorW   { rd, rs1, rs2 }),
                (0x10, true)  => Ok(Instruction::AmominD  { rd, rs1, rs2 }),
                (0x10, false) => Ok(Instruction::AmominW  { rd, rs1, rs2 }),
                (0x14, true)  => Ok(Instruction::AmomaxD  { rd, rs1, rs2 }),
                (0x14, false) => Ok(Instruction::AmomaxW  { rd, rs1, rs2 }),
                (0x18, true)  => Ok(Instruction::AmominuD { rd, rs1, rs2 }),
                (0x18, false) => Ok(Instruction::AmominuW { rd, rs1, rs2 }),
                (0x1C, true)  => Ok(Instruction::AmomaxuD { rd, rs1, rs2 }),
                (0x1C, false) => Ok(Instruction::AmomaxuW { rd, rs1, rs2 }),
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
            }
        },
        0x0F => Ok(Instruction::Fence),
        // System / Zicsr: opcode 0x73. Spec: Unprivileged §9 (CSRs), Privileged §3.3 (mret).
        0x73 => {
            let csr = ((inst >> 20) & 0xfff) as u16;
            match funct3 {
                // funct3=0: PRIV instructions (ecall/ebreak/mret/sret/wfi). Distinguished by full imm.
                0x0 => match (csr, rs1, rd) {
                    (0x000, 0, 0) => Ok(Instruction::Ecall),
                    (0x001, 0, 0) => Ok(Instruction::Ebreak),
                    (0x102, 0, 0) => Ok(Instruction::Sret),
                    (0x302, 0, 0) => Ok(Instruction::Mret),
                    (0x105, 0, 0) => Ok(Instruction::Wfi),
                    // SFENCE.VMA: funct7=0x09, rd must be x0, rs1/rs2 are hints. Priv §4.2.1.
                    (c, _, 0) if c >> 5 == 0x09 => Ok(Instruction::SfenceVma),
                    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
                },
                0x1 => Ok(Instruction::Csrrw  { rd, rs1, csr }),
                0x2 => Ok(Instruction::Csrrs  { rd, rs1, csr }),
                0x3 => Ok(Instruction::Csrrc  { rd, rs1, csr }),
                0x5 => Ok(Instruction::Csrrwi { rd, uimm: rs1 as u32, csr }),
                0x6 => Ok(Instruction::Csrrsi { rd, uimm: rs1 as u32, csr }),
                0x7 => Ok(Instruction::Csrrci { rd, uimm: rs1 as u32, csr }),
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
            }
        },
        // R4-type: Fused Multiply-Add (opcodes 0x43/0x47/0x4B/0x4F).
        // Spec: Unprivileged §11.7 (F), §12.5 (D).
        // inst[31:27]=rs3, inst[26:25]=fmt (0=S,1=D), inst[24:20]=rs2, inst[14:12]=rm.
        0x43 | 0x47 | 0x4B | 0x4F => {
            let rs3 = (funct7 >> 2) as usize;
            let fmt = funct7 & 0x3;
            let rm  = funct3 as u8;
            match (opcode, fmt) {
                (0x43, 0) => Ok(Instruction::FmaddS  { rd, rs1, rs2, rs3, rm }),
                (0x43, 1) => Ok(Instruction::FmaddD  { rd, rs1, rs2, rs3, rm }),
                (0x47, 0) => Ok(Instruction::FmsubS  { rd, rs1, rs2, rs3, rm }),
                (0x47, 1) => Ok(Instruction::FmsubD  { rd, rs1, rs2, rs3, rm }),
                (0x4B, 0) => Ok(Instruction::FnmsubS { rd, rs1, rs2, rs3, rm }),
                (0x4B, 1) => Ok(Instruction::FnmsubD { rd, rs1, rs2, rs3, rm }),
                (0x4F, 0) => Ok(Instruction::FnmaddS { rd, rs1, rs2, rs3, rm }),
                (0x4F, 1) => Ok(Instruction::FnmaddD { rd, rs1, rs2, rs3, rm }),
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
            }
        },
        // OP-FP: opcode 0x53. Spec: Unprivileged §11.6, §12.4.
        // funct7 encodes op+fmt; funct3 is rounding mode (rm) or variant selector.
        0x53 => {
            let rm = funct3 as u8;
            match (funct7, rs2) {
                (0x00, _) => Ok(Instruction::FaddS  { rd, rs1, rs2, rm }),
                (0x01, _) => Ok(Instruction::FaddD  { rd, rs1, rs2, rm }),
                (0x04, _) => Ok(Instruction::FsubS  { rd, rs1, rs2, rm }),
                (0x05, _) => Ok(Instruction::FsubD  { rd, rs1, rs2, rm }),
                (0x08, _) => Ok(Instruction::FmulS  { rd, rs1, rs2, rm }),
                (0x09, _) => Ok(Instruction::FmulD  { rd, rs1, rs2, rm }),
                (0x0C, _) => Ok(Instruction::FdivS  { rd, rs1, rs2, rm }),
                (0x0D, _) => Ok(Instruction::FdivD  { rd, rs1, rs2, rm }),
                (0x2C, 0) => Ok(Instruction::FsqrtS { rd, rs1, rm }),
                (0x2D, 0) => Ok(Instruction::FsqrtD { rd, rs1, rm }),
                // Sign injection: rm field picks variant (0=FSGNJ, 1=FSGNJN, 2=FSGNJX)
                (0x10, _) => match rm {
                    0 => Ok(Instruction::FsgnjS  { rd, rs1, rs2 }),
                    1 => Ok(Instruction::FsgnjnS { rd, rs1, rs2 }),
                    2 => Ok(Instruction::FsgnjxS { rd, rs1, rs2 }),
                    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
                },
                (0x11, _) => match rm {
                    0 => Ok(Instruction::FsgnjD  { rd, rs1, rs2 }),
                    1 => Ok(Instruction::FsgnjnD { rd, rs1, rs2 }),
                    2 => Ok(Instruction::FsgnjxD { rd, rs1, rs2 }),
                    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
                },
                // Min/Max: rm=0→min, rm=1→max
                (0x14, _) => match rm {
                    0 => Ok(Instruction::FminS { rd, rs1, rs2 }),
                    1 => Ok(Instruction::FmaxS { rd, rs1, rs2 }),
                    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
                },
                (0x15, _) => match rm {
                    0 => Ok(Instruction::FminD { rd, rs1, rs2 }),
                    1 => Ok(Instruction::FmaxD { rd, rs1, rs2 }),
                    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
                },
                // Cross-precision conversions (rs2 encodes source fmt)
                (0x20, 1) => Ok(Instruction::FcvtSD { rd, rs1, rm }),
                (0x21, 0) => Ok(Instruction::FcvtDS { rd, rs1, rm }),
                // Float→int: rs2 encodes target integer type (0=W,1=WU,2=L,3=LU)
                (0x60, 0) => Ok(Instruction::FcvtWS  { rd, rs1, rm }),
                (0x60, 1) => Ok(Instruction::FcvtWUS { rd, rs1, rm }),
                (0x60, 2) => Ok(Instruction::FcvtLS  { rd, rs1, rm }),
                (0x60, 3) => Ok(Instruction::FcvtLUS { rd, rs1, rm }),
                (0x61, 0) => Ok(Instruction::FcvtWD  { rd, rs1, rm }),
                (0x61, 1) => Ok(Instruction::FcvtWUD { rd, rs1, rm }),
                (0x61, 2) => Ok(Instruction::FcvtLD  { rd, rs1, rm }),
                (0x61, 3) => Ok(Instruction::FcvtLUD { rd, rs1, rm }),
                // Int→float: rs2 encodes source integer type
                (0x68, 0) => Ok(Instruction::FcvtSW  { rd, rs1, rm }),
                (0x68, 1) => Ok(Instruction::FcvtSWU { rd, rs1, rm }),
                (0x68, 2) => Ok(Instruction::FcvtSL  { rd, rs1, rm }),
                (0x68, 3) => Ok(Instruction::FcvtSLU { rd, rs1, rm }),
                (0x69, 0) => Ok(Instruction::FcvtDW  { rd, rs1, rm }),
                (0x69, 1) => Ok(Instruction::FcvtDWU { rd, rs1, rm }),
                (0x69, 2) => Ok(Instruction::FcvtDL  { rd, rs1, rm }),
                (0x69, 3) => Ok(Instruction::FcvtDLU { rd, rs1, rm }),
                // Comparisons: rm field picks op (0=FLE, 1=FLT, 2=FEQ)
                (0x50, _) => match rm {
                    0 => Ok(Instruction::FleS { rd, rs1, rs2 }),
                    1 => Ok(Instruction::FltS { rd, rs1, rs2 }),
                    2 => Ok(Instruction::FeqS { rd, rs1, rs2 }),
                    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
                },
                (0x51, _) => match rm {
                    0 => Ok(Instruction::FleD { rd, rs1, rs2 }),
                    1 => Ok(Instruction::FltD { rd, rs1, rs2 }),
                    2 => Ok(Instruction::FeqD { rd, rs1, rs2 }),
                    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
                },
                // FMV and FCLASS (rs2 distinguishes fmv rs2=0 from fclass rs2=1)
                (0x70, 0) => Ok(Instruction::FmvXW   { rd, rs1 }),
                (0x70, 1) => Ok(Instruction::FclassS { rd, rs1 }),
                (0x71, 0) => Ok(Instruction::FmvXD   { rd, rs1 }),
                (0x71, 1) => Ok(Instruction::FclassD { rd, rs1 }),
                (0x78, 0) => Ok(Instruction::FmvWX   { rd, rs1 }),
                (0x79, 0) => Ok(Instruction::FmvDX   { rd, rs1 }),
                _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
            }
        },
        _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
    }
}

/// Decode a 16-bit RVC instruction into the equivalent full Instruction.
/// Spec: Unprivileged §16 (C extension, RV64 profile).
/// Returns Err(IllegalInstruction) for reserved encodings.
pub fn decode_rvc(inst: u16) -> Result<Instruction> {
    let w = inst as u32;
    let quad   = w & 0x3;
    let funct3 = (w >> 13) & 0x7;

    // Compressed register field: 3-bit encoding maps to x8..x15.
    let rp = |shift: u32| -> usize { (8 + ((w >> shift) & 0x7)) as usize };

    match quad {
        // ── Quadrant 00 ────────────────────────────────────────────────────────
        0b00 => {
            let rd_p  = rp(2);
            let rs1_p = rp(7);
            match funct3 {
                0b000 => {
                    // C.ADDI4SPN: addi rd', sp, nzuimm  (nzuimm must be nonzero)
                    let uimm = ((w >> 6) & 1) << 2   // inst[6]    → imm[2]
                             | ((w >> 5) & 1) << 3   // inst[5]    → imm[3]
                             | ((w >> 11) & 3) << 4  // inst[12:11]→ imm[5:4]
                             | ((w >> 7) & 0xf) << 6;// inst[10:7] → imm[9:6]
                    if uimm == 0 { return Err(anyhow::Error::new(IllegalInstruction(w))); }
                    Ok(Instruction::Addi { rd: rd_p, rs1: 2, imm: uimm as i64 })
                }
                0b001 => {
                    // C.FLD (RV64, D): fld fd', uimm(rs1') — same offset as C.LD
                    let uimm = ((w >> 10) & 7) << 3  // inst[12:10]→ imm[5:3]
                             | ((w >> 5) & 3) << 6;  // inst[6:5]  → imm[7:6]
                    Ok(Instruction::Fld { fd: rd_p, rs1: rs1_p, imm: uimm as i64 })
                }
                0b010 => {
                    // C.LW: lw rd', uimm(rs1')
                    let uimm = ((w >> 6) & 1) << 2
                             | ((w >> 10) & 7) << 3  // inst[12:10]→ imm[5:3]
                             | ((w >> 5) & 1) << 6;  // inst[5]    → imm[6]
                    Ok(Instruction::Lw { rd: rd_p, rs1: rs1_p, imm: uimm as i64 })
                }
                0b011 => {
                    // C.LD (RV64): ld rd', uimm(rs1')
                    let uimm = ((w >> 10) & 7) << 3  // inst[12:10]→ imm[5:3]
                             | ((w >> 5) & 3) << 6;  // inst[6:5]  → imm[7:6]
                    Ok(Instruction::Ld { rd: rd_p, rs1: rs1_p, imm: uimm as i64 })
                }
                0b101 => {
                    // C.FSD (RV64, D): fsd fs2', uimm(rs1') — same offset as C.SD
                    let fs2_p = rp(2);
                    let uimm = ((w >> 10) & 7) << 3  // inst[12:10]→ imm[5:3]
                             | ((w >> 5) & 3) << 6;  // inst[6:5]  → imm[7:6]
                    Ok(Instruction::Fsd { rs1: rs1_p, fs2: fs2_p, imm: uimm as i64 })
                }
                0b110 => {
                    // C.SW: sw rs2', uimm(rs1')
                    let rs2_p = rp(2);
                    let uimm = ((w >> 6) & 1) << 2
                             | ((w >> 10) & 7) << 3
                             | ((w >> 5) & 1) << 6;
                    Ok(Instruction::Sw { rs1: rs1_p, rs2: rs2_p, imm: uimm as i64 })
                }
                0b111 => {
                    // C.SD (RV64): sd rs2', uimm(rs1')
                    let rs2_p = rp(2);
                    let uimm = ((w >> 10) & 7) << 3
                             | ((w >> 5) & 3) << 6;
                    Ok(Instruction::Sd { rs1: rs1_p, rs2: rs2_p, imm: uimm as i64 })
                }
                _ => Err(anyhow::Error::new(IllegalInstruction(w))),
            }
        }

        // ── Quadrant 01 ────────────────────────────────────────────────────────
        0b01 => {
            let rd_rs1 = ((w >> 7) & 0x1f) as usize;
            // 6-bit signed immediate: bit[12] is sign, bits[6:2] are low 5.
            let imm6 = {
                let raw = ((w >> 12) & 1) << 5 | ((w >> 2) & 0x1f);
                if (raw >> 5) & 1 == 1 { (raw as i64) | (-64i64) } else { raw as i64 }
            };

            match funct3 {
                0b000 => {
                    // C.NOP (rd=0) / C.ADDI: addi rd, rd, sext(imm6)
                    Ok(Instruction::Addi { rd: rd_rs1, rs1: rd_rs1, imm: imm6 })
                }
                0b001 => {
                    // C.ADDIW (RV64): addiw rd, rd, sext(imm6)  (rd must be nonzero)
                    if rd_rs1 == 0 { return Err(anyhow::Error::new(IllegalInstruction(w))); }
                    Ok(Instruction::Addiw { rd: rd_rs1, rs1: rd_rs1, imm: imm6 })
                }
                0b010 => {
                    // C.LI: addi rd, x0, sext(imm6)
                    Ok(Instruction::Addi { rd: rd_rs1, rs1: 0, imm: imm6 })
                }
                0b011 => {
                    if rd_rs1 == 2 {
                        // C.ADDI16SP: addi sp, sp, nzimm (×16 encoded)
                        let raw = ((w >> 12) & 1) << 9   // inst[12] → nzimm[9]
                                | ((w >> 3) & 3) << 7    // inst[4:3] → nzimm[8:7]
                                | ((w >> 5) & 1) << 6    // inst[5]   → nzimm[6]
                                | ((w >> 2) & 1) << 5    // inst[2]   → nzimm[5]
                                | ((w >> 6) & 1) << 4;   // inst[6]   → nzimm[4]
                        let nzimm = if (raw >> 9) & 1 == 1 { (raw as i64) | !0x3ff } else { raw as i64 };
                        if nzimm == 0 { return Err(anyhow::Error::new(IllegalInstruction(w))); }
                        Ok(Instruction::Addi { rd: 2, rs1: 2, imm: nzimm })
                    } else if rd_rs1 != 0 {
                        // C.LUI: lui rd, nzimm[17:12]
                        let raw = ((w >> 12) & 1) << 17
                                | ((w >> 2) & 0x1f) << 12;
                        let nzimm = if (raw >> 17) & 1 == 1 { (raw as i64) | !0x3ffff } else { raw as i64 };
                        if nzimm == 0 { return Err(anyhow::Error::new(IllegalInstruction(w))); }
                        Ok(Instruction::Lui { rd: rd_rs1, imm: nzimm })
                    } else {
                        Err(anyhow::Error::new(IllegalInstruction(w)))
                    }
                }
                0b100 => {
                    let funct2 = (w >> 10) & 0x3;
                    let rd_p  = rp(7);
                    let rs2_p = rp(2);
                    let shamt = (((w >> 12) & 1) << 5 | ((w >> 2) & 0x1f)) as u32;
                    let simm  = { // 6-bit signed for C.ANDI
                        let r = ((w >> 12) & 1) << 5 | ((w >> 2) & 0x1f);
                        if (r >> 5) & 1 == 1 { (r as i64) | (-64i64) } else { r as i64 }
                    };
                    match funct2 {
                        0b00 => Ok(Instruction::Srli { rd: rd_p, rs1: rd_p, shamt }),
                        0b01 => Ok(Instruction::Srai { rd: rd_p, rs1: rd_p, shamt }),
                        0b10 => Ok(Instruction::Andi { rd: rd_p, rs1: rd_p, imm: simm }),
                        0b11 => {
                            let bit12 = (w >> 12) & 1;
                            let funct  = (w >> 5) & 0x3;
                            match (bit12, funct) {
                                (0, 0b00) => Ok(Instruction::Sub  { rd: rd_p, rs1: rd_p, rs2: rs2_p }),
                                (0, 0b01) => Ok(Instruction::Xor  { rd: rd_p, rs1: rd_p, rs2: rs2_p }),
                                (0, 0b10) => Ok(Instruction::Or   { rd: rd_p, rs1: rd_p, rs2: rs2_p }),
                                (0, 0b11) => Ok(Instruction::And  { rd: rd_p, rs1: rd_p, rs2: rs2_p }),
                                (1, 0b00) => Ok(Instruction::Subw { rd: rd_p, rs1: rd_p, rs2: rs2_p }),
                                (1, 0b01) => Ok(Instruction::Addw { rd: rd_p, rs1: rd_p, rs2: rs2_p }),
                                _ => Err(anyhow::Error::new(IllegalInstruction(w))),
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                0b101 => {
                    // C.J: jal x0, sext(imm11)
                    let raw = ((w >> 12) & 1) << 11  // inst[12] → imm[11]
                            | ((w >> 11) & 1) << 4   // inst[11] → imm[4]
                            | ((w >> 9) & 3) << 8    // inst[10:9]→ imm[9:8]
                            | ((w >> 8) & 1) << 10   // inst[8]  → imm[10]
                            | ((w >> 7) & 1) << 6    // inst[7]  → imm[6]
                            | ((w >> 6) & 1) << 7    // inst[6]  → imm[7]
                            | ((w >> 3) & 7) << 1    // inst[5:3]→ imm[3:1]
                            | ((w >> 2) & 1) << 5;   // inst[2]  → imm[5]
                    let imm = if (raw >> 11) & 1 == 1 { (raw as i64) | !0x7ff } else { raw as i64 };
                    Ok(Instruction::Jal { rd: 0, imm })
                }
                0b110 => {
                    // C.BEQZ: beq rs1', x0, sext(imm8)
                    let rs1_p = rp(7);
                    let raw = ((w >> 12) & 1) << 8
                            | ((w >> 10) & 3) << 3   // inst[11:10]→ imm[4:3]
                            | ((w >> 5) & 3) << 6    // inst[6:5]  → imm[7:6]
                            | ((w >> 3) & 3) << 1    // inst[4:3]  → imm[2:1]
                            | ((w >> 2) & 1) << 5;   // inst[2]    → imm[5]
                    let imm = if (raw >> 8) & 1 == 1 { (raw as i64) | !0xff } else { raw as i64 };
                    Ok(Instruction::Beq { rs1: rs1_p, rs2: 0, imm })
                }
                0b111 => {
                    // C.BNEZ: bne rs1', x0, sext(imm8)
                    let rs1_p = rp(7);
                    let raw = ((w >> 12) & 1) << 8
                            | ((w >> 10) & 3) << 3
                            | ((w >> 5) & 3) << 6
                            | ((w >> 3) & 3) << 1
                            | ((w >> 2) & 1) << 5;
                    let imm = if (raw >> 8) & 1 == 1 { (raw as i64) | !0xff } else { raw as i64 };
                    Ok(Instruction::Bne { rs1: rs1_p, rs2: 0, imm })
                }
                _ => Err(anyhow::Error::new(IllegalInstruction(w))),
            }
        }

        // ── Quadrant 10 ────────────────────────────────────────────────────────
        0b10 => {
            let rd_rs1 = ((w >> 7) & 0x1f) as usize;
            let rs2    = ((w >> 2) & 0x1f) as usize;
            match funct3 {
                0b000 => {
                    // C.SLLI: slli rd, rd, shamt
                    let shamt = (((w >> 12) & 1) << 5 | ((w >> 2) & 0x1f)) as u32;
                    Ok(Instruction::Slli { rd: rd_rs1, rs1: rd_rs1, shamt })
                }
                0b001 => {
                    // C.FLDSP (RV64, D): fld rd, uimm(sp) — same offset as C.LDSP
                    let uimm = ((w >> 12) & 1) << 5
                             | ((w >> 5) & 3) << 3   // inst[6:5] → imm[4:3]
                             | ((w >> 2) & 7) << 6;  // inst[4:2] → imm[8:6]
                    Ok(Instruction::Fld { fd: rd_rs1, rs1: 2, imm: uimm as i64 })
                }
                0b010 => {
                    // C.LWSP: lw rd, uimm(sp)  (rd must be nonzero)
                    if rd_rs1 == 0 { return Err(anyhow::Error::new(IllegalInstruction(w))); }
                    let uimm = ((w >> 12) & 1) << 5
                             | ((w >> 4) & 7) << 2   // inst[6:4] → imm[4:2]
                             | ((w >> 2) & 3) << 6;  // inst[3:2] → imm[7:6]
                    Ok(Instruction::Lw { rd: rd_rs1, rs1: 2, imm: uimm as i64 })
                }
                0b011 => {
                    // C.LDSP (RV64): ld rd, uimm(sp)  (rd must be nonzero)
                    if rd_rs1 == 0 { return Err(anyhow::Error::new(IllegalInstruction(w))); }
                    let uimm = ((w >> 12) & 1) << 5
                             | ((w >> 5) & 3) << 3   // inst[6:5] → imm[4:3]
                             | ((w >> 2) & 7) << 6;  // inst[4:2] → imm[8:6]
                    Ok(Instruction::Ld { rd: rd_rs1, rs1: 2, imm: uimm as i64 })
                }
                0b100 => {
                    let bit12 = (w >> 12) & 1;
                    match (bit12, rd_rs1, rs2) {
                        (0, rs1, 0) if rs1 != 0 =>
                            Ok(Instruction::Jalr { rd: 0, rs1: rd_rs1, imm: 0 }), // C.JR
                        (0, rd, _) if rs2 != 0 =>
                            Ok(Instruction::Add { rd, rs1: 0, rs2 }),              // C.MV
                        (1, 0, 0) =>
                            Ok(Instruction::Ebreak),                               // C.EBREAK
                        (1, rs1, 0) if rs1 != 0 =>
                            Ok(Instruction::Jalr { rd: 1, rs1: rd_rs1, imm: 0 }), // C.JALR
                        (1, rd, rs2) if rd != 0 && rs2 != 0 =>
                            Ok(Instruction::Add { rd, rs1: rd_rs1, rs2 }),         // C.ADD
                        _ => Err(anyhow::Error::new(IllegalInstruction(w))),
                    }
                }
                0b101 => {
                    // C.FSDSP (RV64, D): fsd rs2, uimm(sp) — same offset as C.SDSP
                    let uimm = ((w >> 10) & 7) << 3   // inst[12:10]→ imm[5:3]
                             | ((w >> 7) & 7) << 6;   // inst[9:7]  → imm[8:6]
                    Ok(Instruction::Fsd { rs1: 2, fs2: rs2, imm: uimm as i64 })
                }
                0b110 => {
                    // C.SWSP: sw rs2, uimm(sp)
                    let uimm = ((w >> 9) & 0xf) << 2  // inst[12:9]→ imm[5:2]
                             | ((w >> 7) & 3) << 6;   // inst[8:7] → imm[7:6]
                    Ok(Instruction::Sw { rs1: 2, rs2, imm: uimm as i64 })
                }
                0b111 => {
                    // C.SDSP (RV64): sd rs2, uimm(sp)
                    let uimm = ((w >> 10) & 7) << 3   // inst[12:10]→ imm[5:3]
                             | ((w >> 7) & 7) << 6;   // inst[9:7]  → imm[8:6]
                    Ok(Instruction::Sd { rs1: 2, rs2, imm: uimm as i64 })
                }
                _ => Err(anyhow::Error::new(IllegalInstruction(w))),
            }
        }

        _ => unreachable!(), // quad is 2-bit; only 0/1/2 reach here (3 = 32-bit)
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

    // LR.D x3, (x1)  → opcode=0x2F, funct3=3, funct5=0x02, rs2=0, rd=3, rs1=1
    // 0001 0000 0000 0000 1011 0001 1010 1111 = 0x1000_B1AF
    #[test] fn decode_lrd() {
        let inst = decode(0x1000B1AF).unwrap();
        assert_eq!(inst, Instruction::LrD { rd: 3, rs1: 1 });
    }

    // SC.D x3, x2, x1 → funct5=0x03, funct3=3, rd=3, rs1=1, rs2=2
    // 0001 1000 0010 0000 1011 0001 1010 1111 = 0x1820_B1AF
    #[test] fn decode_scd() {
        let inst = decode(0x1820B1AF).unwrap();
        assert_eq!(inst, Instruction::ScD { rd: 3, rs1: 1, rs2: 2 });
    }

    // AMOADD.D x3, x2, (x1) → funct5=0x00, funct3=3, rd=3, rs1=1, rs2=2
    // 0000 0000 0010 0000 1011 0001 1010 1111 = 0x0020_B1AF
    #[test] fn decode_amoaddd() {
        let inst = decode(0x0020B1AF).unwrap();
        assert_eq!(inst, Instruction::AmoaddD { rd: 3, rs1: 1, rs2: 2 });
    }

    #[test]
    fn illegal_instruction_carries_raw_bits() {
        // 0xDEAD_BEFF has opcode 0x7F which is not a valid RISC-V opcode
        let err = decode(0xDEADBEFF).unwrap_err();
        let ill = err.downcast_ref::<IllegalInstruction>()
            .expect("expected IllegalInstruction error");
        assert_eq!(ill.0, 0xDEADBEFF);
    }
}
