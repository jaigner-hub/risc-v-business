use std::collections::HashMap;
use std::mem::offset_of;
use dynasmrt::{dynasm, DynasmApi, DynasmLabelApi, AssemblyOffset, ExecutableBuffer, x64::Assembler};
use crate::cpu::decode::{decode, Instruction};
use crate::cpu::mmu::AccessType;
use crate::cpu::Cpu;

/// Signature of every compiled basic block.
/// - `regs`: pointer to `cpu.regs[0]` — the 32-element u64 register file.
/// - `cpu`: opaque pointer passed through to memory callout helpers.
/// Returns: next guest PC, or `u64::MAX` for slow-path (trap / unhandled instruction).
pub type JitFn = unsafe extern "sysv64" fn(regs: *mut u64, cpu: *mut Cpu) -> u64;

pub struct JitCache {
    blocks: HashMap<u64, (ExecutableBuffer, JitFn)>,
}

/// Load 1 byte (zero-extended to u64). Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load8(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Load) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    cpu.bus.load(pa, 1).unwrap_or(u64::MAX)
}

/// Load 2 bytes (zero-extended to u64). Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load16(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Load) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    cpu.bus.load(pa, 2).unwrap_or(u64::MAX)
}

/// Load 4 bytes (zero-extended to u64). Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load32(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Load) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    cpu.bus.load(pa, 4).unwrap_or(u64::MAX)
}

/// Load 8 bytes. Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load64(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Load) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    cpu.bus.load(pa, 8).unwrap_or(u64::MAX)
}

/// Store 1 byte. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store8(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Store) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    match cpu.bus.store(pa, 1, val) { Ok(_) => 0, Err(_) => u64::MAX }
}

/// Store 2 bytes. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store16(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Store) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    match cpu.bus.store(pa, 2, val) { Ok(_) => 0, Err(_) => u64::MAX }
}

/// Store 4 bytes. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store32(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Store) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    match cpu.bus.store(pa, 4, val) { Ok(_) => 0, Err(_) => u64::MAX }
}

/// Store 8 bytes. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store64(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    let pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, addr, AccessType::Store) {
        Ok(pa) => pa, Err(_) => return u64::MAX,
    };
    match cpu.bus.store(pa, 8, val) { Ok(_) => 0, Err(_) => u64::MAX }
}

type JitLoadFn  = unsafe extern "sysv64" fn(*mut Cpu, u64) -> u64;
type JitStoreFn = unsafe extern "sysv64" fn(*mut Cpu, u64, u64) -> u64;

/// Forces the linker to retain the eight memory-callout helpers above through LTO.
/// JIT-generated machine code calls them by absolute address, so the compiler
/// cannot otherwise see them as live. `#[used]` is only valid on statics, hence
/// these arrays of function-pointer constants.
#[used]
static JIT_LOAD_CALLOUTS: [JitLoadFn; 4] =
    [jit_load8, jit_load16, jit_load32, jit_load64];
#[used]
static JIT_STORE_CALLOUTS: [JitStoreFn; 4] =
    [jit_store8, jit_store16, jit_store32, jit_store64];

// ─── RVC helpers ─────────────────────────────────────────────────────────────

#[inline(always)]
fn sign_ext(val: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((val << shift) as i64) >> shift
}

fn rvc_j_imm(raw: u16) -> i64 {
    // §16.3: inst[12]=imm[11], inst[11]=imm[4], inst[10:9]=imm[9:8],
    //        inst[8]=imm[10], inst[7]=imm[6], inst[6]=imm[7],
    //        inst[5:3]=imm[3:1], inst[2]=imm[5]
    let r = raw as u64;
    let imm = ((r >> 12) & 1) << 11
            | ((r >> 11) & 1) <<  4
            | ((r >>  9) & 3) <<  8
            | ((r >>  8) & 1) << 10
            | ((r >>  7) & 1) <<  6
            | ((r >>  6) & 1) <<  7
            | ((r >>  3) & 7) <<  1
            | ((r >>  2) & 1) <<  5;
    sign_ext(imm, 12)
}

fn rvc_b_imm(raw: u16) -> i64 {
    // §16.3: inst[12]=imm[8], inst[11:10]=imm[4:3], inst[6:5]=imm[7:6],
    //        inst[4:3]=imm[2:1], inst[2]=imm[5]
    let r = raw as u64;
    let imm = ((r >> 12) & 1) << 8
            | ((r >> 10) & 3) << 3
            | ((r >>  5) & 3) << 6
            | ((r >>  3) & 3) << 1
            | ((r >>  2) & 1) << 5;
    sign_ext(imm, 9)
}

enum RvcEffect {
    Seq,       // compiled, advance PC by 2 and continue
    Terminal,  // compiled terminator (branch/jump), end block
    Unhandled, // unrecognised or reserved, fall back to slow path
}

fn emit_rvc(ops: &mut Assembler, raw: u16, guest_pc: u64) -> RvcEffect {
    let quadrant = raw & 0x3;
    let funct3   = (raw >> 13) as usize;
    let next_pc  = guest_pc.wrapping_add(2);

    match quadrant {
        // ── Quadrant 0 ──────────────────────────────────────────────────────
        0b00 => match funct3 {
            0b000 => {
                // C.ADDI4SPN: rd' = x(bits[4:2]+8), uimm = {bits[12:11],bits[10:7],bit[6],bit[5]}
                let rd   = ((raw >> 2) & 0x7) as usize + 8;
                let uimm = (((raw >> 11) & 0x3) as i64) << 4
                         | (((raw >>  7) & 0xf) as i64) << 6
                         | (((raw >>  6) & 0x1) as i64) << 2
                         | (((raw >>  5) & 0x1) as i64) << 3;
                if uimm == 0 { return RvcEffect::Unhandled; } // reserved
                let sp_off = (2 * 8) as i32;
                let rd_off = (rd  * 8) as i32;
                let uimm32 = uimm as i32;
                dynasm!(ops ; .arch x64
                    ; mov rax, QWORD [r15 + sp_off]
                    ; add rax, uimm32
                    ; mov QWORD [r15 + rd_off], rax
                );
                RvcEffect::Seq
            }
            0b011 => {
                // C.LD: ld rd', uimm(rs1')  uimm = {bits[12:10],bits[6:5],00000}
                let rd   = ((raw >> 2) & 0x7) as usize + 8;
                let rs1  = ((raw >> 7) & 0x7) as usize + 8;
                let uimm = (((raw >> 10) & 0x7) as i64) << 3
                         | (((raw >>  5) & 0x3) as i64) << 6;
                emit_load(ops, rd, rs1, uimm, jit_load64, guest_pc);
                RvcEffect::Seq
            }
            0b111 => {
                // C.SD: sd rs2', uimm(rs1')
                let rs2  = ((raw >> 2) & 0x7) as usize + 8;
                let rs1  = ((raw >> 7) & 0x7) as usize + 8;
                let uimm = (((raw >> 10) & 0x7) as i64) << 3
                         | (((raw >>  5) & 0x3) as i64) << 6;
                emit_store(ops, rs1, rs2, uimm, jit_store64, guest_pc);
                RvcEffect::Seq
            }
            _ => RvcEffect::Unhandled,
        },

        // ── Quadrant 1 ──────────────────────────────────────────────────────
        0b01 => match funct3 {
            0b000 => {
                // C.NOP (rd=0) or C.ADDI (rd!=0)
                let rd    = ((raw >> 7) & 0x1f) as usize;
                if rd == 0 { return RvcEffect::Seq; } // C.NOP
                let imm_u = (((raw >> 12) & 0x1) as u64) << 5 | ((raw >> 2) as u64 & 0x1f);
                let imm   = sign_ext(imm_u, 6) as i32;
                let off   = (rd * 8) as i32;
                dynasm!(ops ; .arch x64
                    ; mov rax, QWORD [r15 + off]
                    ; add rax, imm
                    ; mov QWORD [r15 + off], rax
                );
                RvcEffect::Seq
            }
            0b001 => {
                // C.ADDIW: addiw rd, rd, nzimm  (rd != 0)
                let rd = ((raw >> 7) & 0x1f) as usize;
                if rd == 0 { return RvcEffect::Unhandled; } // reserved
                let imm_u = (((raw >> 12) & 0x1) as u64) << 5 | ((raw >> 2) as u64 & 0x1f);
                let imm   = sign_ext(imm_u, 6) as i32;
                let off   = (rd * 8) as i32;
                dynasm!(ops ; .arch x64
                    ; mov eax, DWORD [r15 + off]
                    ; add eax, imm
                    ; movsxd rax, eax
                    ; mov QWORD [r15 + off], rax
                );
                RvcEffect::Seq
            }
            0b010 => {
                // C.LI: addi rd, x0, imm
                let rd    = ((raw >> 7) & 0x1f) as usize;
                if rd == 0 { return RvcEffect::Seq; } // hint
                let imm_u = (((raw >> 12) & 0x1) as u64) << 5 | ((raw >> 2) as u64 & 0x1f);
                let imm   = sign_ext(imm_u, 6) as i32;
                let off   = (rd * 8) as i32;
                dynasm!(ops ; .arch x64
                    ; mov rax, imm  // sign-extends i32 → i64
                    ; mov QWORD [r15 + off], rax
                );
                RvcEffect::Seq
            }
            0b011 => {
                let rd = ((raw >> 7) & 0x1f) as usize;
                if rd == 2 {
                    // C.ADDI16SP: addi x2, x2, nzimm*16
                    // nzimm[9]=bit[12], nzimm[8:7]=bits[4:3], nzimm[6]=bit[5],
                    //          nzimm[5]=bit[2],  nzimm[4]=bit[6]
                    let nzimm_u = (((raw >> 12) & 0x1) as u64) << 9
                                | (((raw >>  3) & 0x3) as u64) << 7
                                | (((raw >>  5) & 0x1) as u64) << 6
                                | (((raw >>  2) & 0x1) as u64) << 5
                                | (((raw >>  6) & 0x1) as u64) << 4;
                    let nzimm = sign_ext(nzimm_u, 10) as i32;
                    if nzimm == 0 { return RvcEffect::Unhandled; } // reserved
                    let sp_off = (2 * 8) as i32;
                    dynasm!(ops ; .arch x64
                        ; mov rax, QWORD [r15 + sp_off]
                        ; add rax, nzimm
                        ; mov QWORD [r15 + sp_off], rax
                    );
                    RvcEffect::Seq
                } else if rd != 0 {
                    // C.LUI: lui rd, nzimm  (stored as sign_ext(imm6,6)<<12)
                    let nzimm_u = (((raw >> 12) & 0x1) as u64) << 5 | ((raw >> 2) as u64 & 0x1f);
                    let nzimm   = sign_ext(nzimm_u, 6);
                    if nzimm == 0 { return RvcEffect::Unhandled; } // reserved
                    let val = nzimm << 12;
                    let off = (rd * 8) as i32;
                    dynasm!(ops ; .arch x64
                        ; mov rax, QWORD val
                        ; mov QWORD [r15 + off], rax
                    );
                    RvcEffect::Seq
                } else {
                    RvcEffect::Unhandled // rd=0 HINTS
                }
            }
            0b100 => {
                let op2   = (raw >> 10) & 0x3;
                let rd    = ((raw >> 7) & 0x7) as usize + 8;
                let off   = (rd * 8) as i32;
                match op2 {
                    0b00 => {
                        // C.SRLI
                        let shamt = (((raw >> 12) & 0x1) << 5 | ((raw >> 2) & 0x1f)) as i8;
                        dynasm!(ops ; .arch x64
                            ; mov rax, QWORD [r15 + off]
                            ; shr rax, shamt
                            ; mov QWORD [r15 + off], rax
                        );
                        RvcEffect::Seq
                    }
                    0b01 => {
                        // C.SRAI
                        let shamt = (((raw >> 12) & 0x1) << 5 | ((raw >> 2) & 0x1f)) as i8;
                        dynasm!(ops ; .arch x64
                            ; mov rax, QWORD [r15 + off]
                            ; sar rax, shamt
                            ; mov QWORD [r15 + off], rax
                        );
                        RvcEffect::Seq
                    }
                    0b10 => {
                        // C.ANDI
                        let imm_u = (((raw >> 12) & 0x1) as u64) << 5 | ((raw >> 2) as u64 & 0x1f);
                        let imm   = sign_ext(imm_u, 6) as i32;
                        dynasm!(ops ; .arch x64
                            ; mov rax, QWORD [r15 + off]
                            ; and rax, imm
                            ; mov QWORD [r15 + off], rax
                        );
                        RvcEffect::Seq
                    }
                    0b11 => {
                        let bit12   = (raw >> 12) & 0x1;
                        let op_sel  = (raw >> 5) & 0x3;
                        let rs2     = ((raw >> 2) & 0x7) as usize + 8;
                        let rs2_off = (rs2 * 8) as i32;
                        if bit12 == 0 {
                            match op_sel {
                                0b00 => { dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15+off] ; sub rax, QWORD [r15+rs2_off] ; mov QWORD [r15+off], rax); RvcEffect::Seq } // C.SUB
                                0b01 => { dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15+off] ; xor rax, QWORD [r15+rs2_off] ; mov QWORD [r15+off], rax); RvcEffect::Seq } // C.XOR
                                0b10 => { dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15+off] ; or  rax, QWORD [r15+rs2_off] ; mov QWORD [r15+off], rax); RvcEffect::Seq } // C.OR
                                0b11 => { dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15+off] ; and rax, QWORD [r15+rs2_off] ; mov QWORD [r15+off], rax); RvcEffect::Seq } // C.AND
                                _    => unreachable!(),
                            }
                        } else {
                            match op_sel {
                                0b00 => { // C.SUBW
                                    dynasm!(ops ; .arch x64 ; mov eax, DWORD [r15+off] ; sub eax, DWORD [r15+rs2_off] ; movsxd rax, eax ; mov QWORD [r15+off], rax);
                                    RvcEffect::Seq
                                }
                                0b01 => { // C.ADDW
                                    dynasm!(ops ; .arch x64 ; mov eax, DWORD [r15+off] ; add eax, DWORD [r15+rs2_off] ; movsxd rax, eax ; mov QWORD [r15+off], rax);
                                    RvcEffect::Seq
                                }
                                _ => RvcEffect::Unhandled, // reserved
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
            0b101 => {
                // C.J: jal x0, offset
                let target = guest_pc.wrapping_add(rvc_j_imm(raw) as u64);
                emit_return(ops, target);
                RvcEffect::Terminal
            }
            0b110 => {
                // C.BEQZ: beq rs1', x0, offset
                let rs1     = ((raw >> 7) & 0x7) as usize + 8;
                let imm     = rvc_b_imm(raw);
                let taken   = guest_pc.wrapping_add(imm as u64);
                let fall    = next_pc;
                let rs1_off = (rs1 * 8) as i32;
                let lbl     = ops.new_dynamic_label();
                dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15 + rs1_off] ; test rax, rax ; jz =>lbl);
                emit_return(ops, fall);
                dynasm!(ops ; .arch x64 ; =>lbl);
                emit_return(ops, taken);
                RvcEffect::Terminal
            }
            0b111 => {
                // C.BNEZ: bne rs1', x0, offset
                let rs1     = ((raw >> 7) & 0x7) as usize + 8;
                let imm     = rvc_b_imm(raw);
                let taken   = guest_pc.wrapping_add(imm as u64);
                let fall    = next_pc;
                let rs1_off = (rs1 * 8) as i32;
                let lbl     = ops.new_dynamic_label();
                dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15 + rs1_off] ; test rax, rax ; jnz =>lbl);
                emit_return(ops, fall);
                dynasm!(ops ; .arch x64 ; =>lbl);
                emit_return(ops, taken);
                RvcEffect::Terminal
            }
            _ => RvcEffect::Unhandled,
        },

        // ── Quadrant 2 ──────────────────────────────────────────────────────
        0b10 => match funct3 {
            0b000 => {
                // C.SLLI: slli rd, rd, shamt  (rd != 0)
                let rd    = ((raw >> 7) & 0x1f) as usize;
                if rd == 0 { return RvcEffect::Seq; } // hint
                let shamt = (((raw >> 12) & 0x1) << 5 | ((raw >> 2) & 0x1f)) as i8;
                let off   = (rd * 8) as i32;
                dynasm!(ops ; .arch x64
                    ; mov rax, QWORD [r15 + off]
                    ; shl rax, shamt
                    ; mov QWORD [r15 + off], rax
                );
                RvcEffect::Seq
            }
            0b011 => {
                // C.LDSP: ld rd, uimm(x2)  (rd != 0)
                // uimm[5]=bit[12], uimm[4:3]=bits[6:5], uimm[8:6]=bits[4:2]
                let rd   = ((raw >> 7) & 0x1f) as usize;
                if rd == 0 { return RvcEffect::Unhandled; } // reserved
                let uimm = (((raw >> 12) & 0x1) as i64) << 5
                         | (((raw >>  5) & 0x3) as i64) << 3
                         | (((raw >>  2) & 0x7) as i64) << 6;
                emit_load(ops, rd, 2, uimm, jit_load64, guest_pc);
                RvcEffect::Seq
            }
            0b100 => {
                let bit12  = (raw >> 12) & 0x1;
                let rs1_rd = ((raw >> 7) & 0x1f) as usize;
                let rs2    = ((raw >> 2) & 0x1f) as usize;
                if bit12 == 0 && rs2 == 0 {
                    // C.JR: jalr x0, 0(rs1)
                    if rs1_rd == 0 { return RvcEffect::Unhandled; } // reserved
                    let rs1_off = (rs1_rd * 8) as i32;
                    dynasm!(ops ; .arch x64
                        ; mov rcx, QWORD [r15 + rs1_off]
                        ; and rcx, DWORD -2
                        ; add rsp, 8
                        ; pop r14
                        ; pop r15
                        ; mov rax, rcx
                        ; ret
                    );
                    RvcEffect::Terminal
                } else if bit12 == 0 {
                    // C.MV: add rd, x0, rs2
                    if rs1_rd == 0 { return RvcEffect::Seq; } // hint
                    let rd_off  = (rs1_rd * 8) as i32;
                    let rs2_off = (rs2    * 8) as i32;
                    dynasm!(ops ; .arch x64
                        ; mov rax, QWORD [r15 + rs2_off]
                        ; mov QWORD [r15 + rd_off], rax
                    );
                    RvcEffect::Seq
                } else if rs2 == 0 {
                    // C.JALR: jalr x1, 0(rs1)  (rs1!=0); rs1=0 is C.EBREAK
                    if rs1_rd == 0 { return RvcEffect::Unhandled; } // C.EBREAK → interp
                    let rs1_off = (rs1_rd * 8) as i32;
                    let ra_off  = 8i32; // x1 = ra
                    let link_pc = next_pc as i64;
                    dynasm!(ops ; .arch x64
                        ; mov rcx, QWORD [r15 + rs1_off]
                        ; mov rax, QWORD link_pc
                        ; mov QWORD [r15 + ra_off], rax
                        ; and rcx, DWORD -2
                        ; add rsp, 8
                        ; pop r14
                        ; pop r15
                        ; mov rax, rcx
                        ; ret
                    );
                    RvcEffect::Terminal
                } else {
                    // C.ADD: add rd, rd, rs2
                    if rs1_rd == 0 { return RvcEffect::Seq; } // hint
                    let rd_off  = (rs1_rd * 8) as i32;
                    let rs2_off = (rs2    * 8) as i32;
                    dynasm!(ops ; .arch x64
                        ; mov rax, QWORD [r15 + rd_off]
                        ; add rax, QWORD [r15 + rs2_off]
                        ; mov QWORD [r15 + rd_off], rax
                    );
                    RvcEffect::Seq
                }
            }
            0b111 => {
                // C.SDSP: sd rs2, uimm(x2)
                // uimm[5:3]=bits[12:10], uimm[8:6]=bits[9:7]
                let rs2  = ((raw >> 2) & 0x1f) as usize;
                let uimm = (((raw >> 10) & 0x7) as i64) << 3
                         | (((raw >>  7) & 0x7) as i64) << 6;
                emit_store(ops, 2, rs2, uimm, jit_store64, guest_pc);
                RvcEffect::Seq
            }
            _ => RvcEffect::Unhandled,
        },

        _ => RvcEffect::Unhandled, // quadrant 11 = 32-bit, handled in compile()
    }
}

// ─── Block emitters ──────────────────────────────────────────────────────────
//
// Calling convention:
//   Entry: rdi = &mut cpu.regs[0], rsi = *mut Cpu
//   Inside block: r15 = regs base, r14 = cpu ptr (both callee-saved)
//   Return: rax = next guest PC, or u64::MAX on slow path

fn emit_prologue(ops: &mut Assembler) {
    // After the caller's CALL, RSP ≡ 8 mod 16 (return address pushed).
    // Two callee-save pushes bring RSP to 16N-24 ≡ 8 mod 16.
    // `sub rsp, 8` pads to 16N-32 ≡ 0 mod 16 so every nested CALL in the block
    // obeys the SysV ABI 16-byte alignment requirement.
    dynasm!(ops
        ; .arch x64
        ; push r15
        ; push r14
        ; sub rsp, 8
        ; mov r15, rdi
        ; mov r14, rsi
    );
}

fn emit_return(ops: &mut Assembler, next_pc: u64) {
    let pc_val = next_pc as i64;
    dynasm!(ops
        ; .arch x64
        ; add rsp, 8
        ; pop r14
        ; pop r15
        ; mov rax, QWORD pc_val
        ; ret
    );
}

fn emit_slow_path(ops: &mut Assembler) {
    dynasm!(ops
        ; .arch x64
        ; add rsp, 8
        ; pop r14
        ; pop r15
        ; mov rax, QWORD -1i64
        ; ret
    );
}

// Like emit_slow_path but also writes `fault_pc` into cpu.pc (r14 points to Cpu) so
// that the main loop's cpu.step() executes the correct faulting instruction, not the
// block's start PC. Used by load/store callout fault paths where inst_count > 0.
fn emit_fault_return(ops: &mut Assembler, fault_pc: u64) {
    let pc_offset = offset_of!(Cpu, pc) as i32;
    let pc_val    = fault_pc as i64;
    dynasm!(ops
        ; .arch x64
        ; mov rax, QWORD pc_val
        ; mov QWORD [r14 + pc_offset], rax  // cpu.pc = fault_pc
        ; add rsp, 8
        ; pop r14
        ; pop r15
        ; mov rax, QWORD -1i64              // u64::MAX: call cpu.step() in main loop
        ; ret
    );
}

fn emit_load(
    ops: &mut Assembler,
    rd: usize,
    rs1: usize,
    imm: i64,
    helper: unsafe extern "sysv64" fn(*mut Cpu, u64) -> u64,
    guest_pc: u64,
) {
    let rs1_off = (rs1 * 8) as i32;
    let rd_off  = (rd  * 8) as i32;
    let imm32   = imm as i32;
    let helper_addr = helper as i64;
    let fault_label = ops.new_dynamic_label();
    let skip_fault  = ops.new_dynamic_label();
    dynasm!(ops
        ; .arch x64
        ; mov rsi, QWORD [r15 + rs1_off]
        ; add rsi, imm32
        ; mov rdi, r14
        ; mov rax, QWORD helper_addr
        ; call rax
        ; cmp rax, -1i32
        ; je =>fault_label
    );
    if rd != 0 {
        dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
    }
    dynasm!(ops
        ; .arch x64
        ; jmp =>skip_fault
        ; =>fault_label
    );
    emit_fault_return(ops, guest_pc);
    dynasm!(ops ; .arch x64 ; =>skip_fault);
}

fn emit_store(
    ops: &mut Assembler,
    rs1: usize,
    rs2: usize,
    imm: i64,
    helper: unsafe extern "sysv64" fn(*mut Cpu, u64, u64) -> u64,
    guest_pc: u64,
) {
    let rs1_off = (rs1 * 8) as i32;
    let rs2_off = (rs2 * 8) as i32;
    let imm32   = imm as i32;
    let helper_addr = helper as i64;
    let fault_label = ops.new_dynamic_label();
    let skip_fault  = ops.new_dynamic_label();
    dynasm!(ops
        ; .arch x64
        ; mov rsi, QWORD [r15 + rs1_off]
        ; add rsi, imm32
        ; mov rdx, QWORD [r15 + rs2_off]
        ; mov rdi, r14
        ; mov rax, QWORD helper_addr
        ; call rax
        ; cmp rax, -1i32
        ; je =>fault_label
    );
    dynasm!(ops
        ; .arch x64
        ; jmp =>skip_fault
        ; =>fault_label
    );
    emit_fault_return(ops, guest_pc);
    dynasm!(ops ; .arch x64 ; =>skip_fault);
}

/// R-type op pattern: rax = regs[rs1], rcx = regs[rs2], op(rax, rcx), regs[rd] = rax (if rd!=0).
fn emit_r_op<F: FnOnce(&mut Assembler)>(
    ops: &mut Assembler, rd: usize, rs1: usize, rs2: usize, op: F
) {
    let rs1_off = (rs1 * 8) as i32;
    let rs2_off = (rs2 * 8) as i32;
    let rd_off  = (rd  * 8) as i32;
    dynasm!(ops
        ; .arch x64
        ; mov rax, QWORD [r15 + rs1_off]
        ; mov rcx, QWORD [r15 + rs2_off]
    );
    op(ops);
    if rd != 0 {
        dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
    }
}

/// I-type op pattern: rax = regs[rs1], op(rax, imm32), regs[rd] = rax (if rd!=0).
fn emit_i_op<F: FnOnce(&mut Assembler, i32)>(
    ops: &mut Assembler, rd: usize, rs1: usize, imm: i64, op: F
) {
    let rs1_off = (rs1 * 8) as i32;
    let rd_off  = (rd  * 8) as i32;
    let imm32   = imm as i32;
    dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15 + rs1_off]);
    op(ops, imm32);
    if rd != 0 {
        dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
    }
}

/// W-variant pattern: 32-bit op then sign-extend to 64.
fn emit_w_op<F: FnOnce(&mut Assembler)>(
    ops: &mut Assembler, rd: usize, rs1: usize, rs2: usize, op: F
) {
    let rs1_off = (rs1 * 8) as i32;
    let rs2_off = (rs2 * 8) as i32;
    let rd_off  = (rd  * 8) as i32;
    dynasm!(ops
        ; .arch x64
        ; mov eax, DWORD [r15 + rs1_off]
        ; mov ecx, DWORD [r15 + rs2_off]
    );
    op(ops);
    dynasm!(ops ; .arch x64 ; movsxd rax, eax);
    if rd != 0 {
        dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
    }
}

impl JitCache {
    pub fn new() -> Self {
        Self { blocks: HashMap::new() }
    }

    /// Look up a compiled block for `pc`. Returns `None` if not yet compiled.
    pub fn get(&self, pc: u64) -> Option<JitFn> {
        self.blocks.get(&pc).map(|&(_, f)| f)
    }

    /// Flush the entire block cache (called on satp write and sfence.vma).
    pub fn invalidate(&mut self) {
        self.blocks.clear();
    }

    /// Number of compiled blocks currently in the cache.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Compile the basic block starting at guest virtual address `start_pc`.
    /// No-op if the block is already cached. The block ends at the first
    /// unhandled instruction (slow-path return), or after 64 instructions
    /// (fall-through return to next sequential PC).
    pub fn compile(&mut self, cpu: &mut Cpu, start_pc: u64) {
        if self.blocks.contains_key(&start_pc) { return; }

        // Translate start_pc before fetching the first instruction. In M-mode or with
        // satp.MODE=0 this is a passthrough. Returns early if the VA is unmapped.
        let start_pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, start_pc, AccessType::Fetch) {
            Ok(pa) => pa, Err(_) => return,
        };
        if cpu.bus.load(start_pa, 4).is_err() { return; }

        let mut ops = Assembler::new().unwrap();
        let entry: AssemblyOffset = ops.offset();
        emit_prologue(&mut ops);

        let mut guest_pc = start_pc;
        let mut inst_count = 0u32;

        loop {
            macro_rules! block_exit {
                () => {
                    if inst_count > 0 {
                        emit_return(&mut ops, guest_pc);
                    } else {
                        emit_slow_path(&mut ops);
                    }
                };
            }

            let inst_pa = match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, guest_pc, AccessType::Fetch) {
                Ok(pa) => pa,
                Err(_) => { block_exit!(); break; }
            };
            let raw4 = match cpu.bus.load(inst_pa, 4) {
                Ok(v)  => v as u32,
                Err(_) => { block_exit!(); break; }
            };

            // RVC (16-bit compressed instructions)
            if raw4 & 0x3 != 0x3 {
                let raw16 = (raw4 & 0xffff) as u16;
                match emit_rvc(&mut ops, raw16, guest_pc) {
                    RvcEffect::Seq => {
                        guest_pc = guest_pc.wrapping_add(2);
                        inst_count += 1;
                        if inst_count >= 128 {
                            emit_return(&mut ops, guest_pc);
                            break;
                        }
                        continue;
                    }
                    RvcEffect::Terminal => { inst_count += 1; break; }
                    RvcEffect::Unhandled => { block_exit!(); break; }
                }
            }

            let (inst, inst_size): (Instruction, u64) = match decode(raw4) {
                Ok(i)  => (i, 4),
                Err(_) => { block_exit!(); break; }
            };

            let next_seq = guest_pc.wrapping_add(inst_size);

            match inst {
                // R-type
                Instruction::Add { rd, rs1, rs2 } => {
                    emit_r_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; add rax, rcx);
                    });
                }
                Instruction::Sub { rd, rs1, rs2 } => {
                    emit_r_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; sub rax, rcx);
                    });
                }
                Instruction::And { rd, rs1, rs2 } => {
                    emit_r_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; and rax, rcx);
                    });
                }
                Instruction::Or { rd, rs1, rs2 } => {
                    emit_r_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; or rax, rcx);
                    });
                }
                Instruction::Xor { rd, rs1, rs2 } => {
                    emit_r_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; xor rax, rcx);
                    });
                }
                Instruction::Sll { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        ; and rcx, 63
                        ; shl rax, cl
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Srl { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        ; and rcx, 63
                        ; shr rax, cl
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Sra { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        ; and rcx, 63
                        ; sar rax, cl
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Slt { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; cmp rax, QWORD [r15 + rs2_off]
                        ; setl al
                        ; movzx rax, al
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Sltu { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; cmp rax, QWORD [r15 + rs2_off]
                        ; setb al
                        ; movzx rax, al
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }

                // I-type arithmetic
                Instruction::Addi { rd, rs1, imm } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let imm32   = imm as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; add rax, imm32
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Andi { rd, rs1, imm } => {
                    emit_i_op(&mut ops, rd, rs1, imm, |ops, imm32| {
                        dynasm!(ops ; .arch x64 ; and rax, imm32);
                    });
                }
                Instruction::Ori { rd, rs1, imm } => {
                    emit_i_op(&mut ops, rd, rs1, imm, |ops, imm32| {
                        dynasm!(ops ; .arch x64 ; or rax, imm32);
                    });
                }
                Instruction::Xori { rd, rs1, imm } => {
                    emit_i_op(&mut ops, rd, rs1, imm, |ops, imm32| {
                        dynasm!(ops ; .arch x64 ; xor rax, imm32);
                    });
                }
                Instruction::Slti { rd, rs1, imm } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let imm32   = imm as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; cmp rax, imm32
                        ; setl al
                        ; movzx rax, al
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Sltiu { rd, rs1, imm } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let imm32   = imm as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; cmp rax, imm32
                        ; setb al
                        ; movzx rax, al
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Slli { rd, rs1, shamt } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let sh      = shamt as i8;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; shl rax, sh
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Srli { rd, rs1, shamt } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let sh      = shamt as i8;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; shr rax, sh
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Srai { rd, rs1, shamt } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let sh      = shamt as i8;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; sar rax, sh
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }

                // W-variants R-type
                Instruction::Addw { rd, rs1, rs2 } => {
                    emit_w_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; add eax, ecx);
                    });
                }
                Instruction::Subw { rd, rs1, rs2 } => {
                    emit_w_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; sub eax, ecx);
                    });
                }
                Instruction::Sllw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; mov ecx, DWORD [r15 + rs2_off]
                        ; and ecx, 31
                        ; shl eax, cl
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Srlw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; mov ecx, DWORD [r15 + rs2_off]
                        ; and ecx, 31
                        ; shr eax, cl
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Sraw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; mov ecx, DWORD [r15 + rs2_off]
                        ; and ecx, 31
                        ; sar eax, cl
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }

                // W-variants I-type
                Instruction::Addiw { rd, rs1, imm } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let imm32   = imm as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; add eax, imm32
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Slliw { rd, rs1, shamt } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let sh = shamt as i8;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; shl eax, sh
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Srliw { rd, rs1, shamt } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let sh = shamt as i8;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; shr eax, sh
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Sraiw { rd, rs1, shamt } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let sh = shamt as i8;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; sar eax, sh
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }

                // Upper immediates
                Instruction::Lui { rd, imm } => {
                    if rd != 0 {
                        let rd_off  = (rd * 8) as i32;
                        let imm_val = imm as i64;
                        dynasm!(ops
                            ; .arch x64
                            ; mov rax, QWORD imm_val
                            ; mov QWORD [r15 + rd_off], rax
                        );
                    }
                }
                Instruction::Auipc { rd, imm } => {
                    if rd != 0 {
                        let rd_off = (rd * 8) as i32;
                        let result = (guest_pc as i64).wrapping_add(imm);
                        dynasm!(ops
                            ; .arch x64
                            ; mov rax, QWORD result
                            ; mov QWORD [r15 + rd_off], rax
                        );
                    }
                }

                // M-extension: multiply
                Instruction::Mul { rd, rs1, rs2 } => {
                    emit_r_op(&mut ops, rd, rs1, rs2, |ops| {
                        dynasm!(ops ; .arch x64 ; imul rax, rcx);
                    });
                }
                Instruction::Mulh { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        ; imul rcx          // rdx:rax = signed product; upper bits in rdx
                        ; mov rax, rdx
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Mulhu { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        ; mul rcx            // rdx:rax = unsigned product
                        ; mov rax, rdx
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Mulhsu { .. } => {
                    block_exit!();
                    break;
                }
                Instruction::Mulw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; imul eax, DWORD [r15 + rs2_off]
                        ; movsxd rax, eax
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }

                // M-extension: divide / remainder (signed 64-bit)
                Instruction::Div { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        // Division by zero: return -1
                        ; test rcx, rcx
                        ; jnz >not_zero
                        ; mov rax, QWORD -1i64
                        ; jmp =>done
                        ;not_zero:
                        // Overflow: i64::MIN / -1 → return i64::MIN
                        ; mov rdx, QWORD i64::MIN as i64
                        ; cmp rax, rdx
                        ; jne >no_overflow
                        ; cmp rcx, -1i32
                        ; jne >no_overflow
                        ; jmp =>done          // rax already = i64::MIN
                        ;no_overflow:
                        ; cqo                  // sign-extend rax → rdx:rax
                        ; idiv rcx
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Divu { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        ; test rcx, rcx
                        ; jnz >not_zero
                        ; mov rax, QWORD -1i64   // u64::MAX
                        ; jmp =>done
                        ;not_zero:
                        ; xor rdx, rdx
                        ; div rcx
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Rem { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        // Div-by-zero: remainder = dividend
                        ; test rcx, rcx
                        ; jnz >not_zero
                        ; jmp =>done          // rax already = dividend
                        ;not_zero:
                        // Overflow: i64::MIN % -1 = 0
                        ; mov rdx, QWORD i64::MIN as i64
                        ; cmp rax, rdx
                        ; jne >no_overflow
                        ; cmp rcx, -1i32
                        ; jne >no_overflow
                        ; xor eax, eax
                        ; jmp =>done
                        ;no_overflow:
                        ; cqo
                        ; idiv rcx
                        ; mov rax, rdx        // remainder in rdx
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Remu { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; mov rcx, QWORD [r15 + rs2_off]
                        ; test rcx, rcx
                        ; jnz >not_zero
                        ; jmp =>done          // remu div-by-zero = dividend
                        ;not_zero:
                        ; xor rdx, rdx
                        ; div rcx
                        ; mov rax, rdx
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }

                // M-extension: 32-bit W-variants
                Instruction::Divw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; movsxd rax, DWORD [r15 + rs1_off]
                        ; movsxd rcx, DWORD [r15 + rs2_off]
                        ; test rcx, rcx
                        ; jnz >not_zero
                        ; mov rax, QWORD -1i64       // div-by-zero: -1 (sign-extended)
                        ; jmp =>done
                        ;not_zero:
                        ; mov rdx, QWORD i32::MIN as i64
                        ; cmp rax, rdx
                        ; jne >no_overflow
                        ; cmp rcx, -1i32
                        ; jne >no_overflow
                        ; jmp =>done           // overflow: return i64::MIN already in rax
                        ;no_overflow:
                        ; cqo
                        ; idiv rcx
                        ; movsxd rax, eax      // sign-extend 32-bit quotient to 64 bits
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Divuw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]    // zero-extend
                        ; mov ecx, DWORD [r15 + rs2_off]    // zero-extend
                        ; test ecx, ecx
                        ; jnz >not_zero
                        ; mov rax, QWORD -1i64
                        ; jmp =>done
                        ;not_zero:
                        ; xor edx, edx
                        ; div ecx
                        ; movsxd rax, eax
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Remw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; movsxd rax, DWORD [r15 + rs1_off]
                        ; movsxd rcx, DWORD [r15 + rs2_off]
                        ; test rcx, rcx
                        ; jnz >not_zero
                        ; movsxd rax, eax      // div-by-zero: remainder = dividend sign-extended
                        ; jmp =>done
                        ;not_zero:
                        ; mov rdx, QWORD i32::MIN as i64
                        ; cmp rax, rdx
                        ; jne >no_overflow
                        ; cmp rcx, -1i32
                        ; jne >no_overflow
                        ; xor eax, eax         // overflow: remainder = 0
                        ; jmp =>done
                        ;no_overflow:
                        ; cqo
                        ; idiv rcx
                        ; movsxd rax, edx      // sign-extend 32-bit remainder
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }
                Instruction::Remuw { rd, rs1, rs2 } => {
                    let rs1_off = (rs1 * 8) as i32;
                    let rs2_off = (rs2 * 8) as i32;
                    let rd_off  = (rd  * 8) as i32;
                    let done    = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; mov eax, DWORD [r15 + rs1_off]
                        ; mov ecx, DWORD [r15 + rs2_off]
                        ; test ecx, ecx
                        ; jnz >not_zero
                        ; movsxd rax, eax      // div-by-zero: remainder = dividend sign-extended
                        ; jmp =>done
                        ;not_zero:
                        ; xor edx, edx
                        ; div ecx
                        ; movsxd rax, edx
                        ; =>done
                    );
                    if rd != 0 { dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax); }
                }

                // ── Loads ──────────────────────────────────────────────────────
                Instruction::Lb { rd, rs1, imm } => {
                    emit_load(&mut ops, rd, rs1, imm as i64, jit_load8, guest_pc);
                    if rd != 0 {
                        let rd_off = (rd * 8) as i32;
                        dynasm!(ops ; .arch x64
                            ; movsx rax, BYTE [r15 + rd_off]
                            ; mov QWORD [r15 + rd_off], rax
                        );
                    }
                }
                Instruction::Lh { rd, rs1, imm } => {
                    emit_load(&mut ops, rd, rs1, imm as i64, jit_load16, guest_pc);
                    if rd != 0 {
                        let rd_off = (rd * 8) as i32;
                        dynasm!(ops ; .arch x64
                            ; movsx rax, WORD [r15 + rd_off]
                            ; mov QWORD [r15 + rd_off], rax
                        );
                    }
                }
                Instruction::Lw { rd, rs1, imm } => {
                    emit_load(&mut ops, rd, rs1, imm as i64, jit_load32, guest_pc);
                    if rd != 0 {
                        let rd_off = (rd * 8) as i32;
                        dynasm!(ops ; .arch x64
                            ; movsxd rax, DWORD [r15 + rd_off]
                            ; mov QWORD [r15 + rd_off], rax
                        );
                    }
                }
                Instruction::Ld { rd, rs1, imm } => {
                    emit_load(&mut ops, rd, rs1, imm as i64, jit_load64, guest_pc);
                }
                Instruction::Lbu { rd, rs1, imm } => {
                    emit_load(&mut ops, rd, rs1, imm as i64, jit_load8, guest_pc);
                }
                Instruction::Lhu { rd, rs1, imm } => {
                    emit_load(&mut ops, rd, rs1, imm as i64, jit_load16, guest_pc);
                }
                Instruction::Lwu { rd, rs1, imm } => {
                    emit_load(&mut ops, rd, rs1, imm as i64, jit_load32, guest_pc);
                }

                // ── Stores ─────────────────────────────────────────────────────
                Instruction::Sb { rs1, rs2, imm } => {
                    emit_store(&mut ops, rs1, rs2, imm as i64, jit_store8, guest_pc);
                }
                Instruction::Sh { rs1, rs2, imm } => {
                    emit_store(&mut ops, rs1, rs2, imm as i64, jit_store16, guest_pc);
                }
                Instruction::Sw { rs1, rs2, imm } => {
                    emit_store(&mut ops, rs1, rs2, imm as i64, jit_store32, guest_pc);
                }
                Instruction::Sd { rs1, rs2, imm } => {
                    emit_store(&mut ops, rs1, rs2, imm as i64, jit_store64, guest_pc);
                }

                // ── Conditional branches (end block) ──────────────────────────
                Instruction::Beq  { rs1, rs2, imm } |
                Instruction::Bne  { rs1, rs2, imm } |
                Instruction::Blt  { rs1, rs2, imm } |
                Instruction::Bge  { rs1, rs2, imm } |
                Instruction::Bltu { rs1, rs2, imm } |
                Instruction::Bgeu { rs1, rs2, imm } => {
                    let taken_pc  = guest_pc.wrapping_add(imm as u64) as i64;
                    let fall_pc   = next_seq as i64;
                    let rs1_off   = (rs1 * 8) as i32;
                    let rs2_off   = (rs2 * 8) as i32;
                    let taken_lbl = ops.new_dynamic_label();
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD [r15 + rs1_off]
                        ; cmp rax, QWORD [r15 + rs2_off]
                    );
                    match inst {
                        Instruction::Beq  { .. } => dynasm!(ops ; .arch x64 ; je  =>taken_lbl),
                        Instruction::Bne  { .. } => dynasm!(ops ; .arch x64 ; jne =>taken_lbl),
                        Instruction::Blt  { .. } => dynasm!(ops ; .arch x64 ; jl  =>taken_lbl),
                        Instruction::Bge  { .. } => dynasm!(ops ; .arch x64 ; jge =>taken_lbl),
                        Instruction::Bltu { .. } => dynasm!(ops ; .arch x64 ; jb  =>taken_lbl),
                        Instruction::Bgeu { .. } => dynasm!(ops ; .arch x64 ; jae =>taken_lbl),
                        _ => unreachable!(),
                    }
                    // Fall-through path
                    emit_return(&mut ops, fall_pc as u64);
                    // Taken path
                    dynasm!(ops ; .arch x64 ; =>taken_lbl);
                    emit_return(&mut ops, taken_pc as u64);
                    inst_count += 1;
                    break;
                }

                // ── JAL (end block) ────────────────────────────────────────────
                Instruction::Jal { rd, imm } => {
                    let link_pc   = next_seq as i64;
                    let target_pc = guest_pc.wrapping_add(imm as u64) as i64;
                    if rd != 0 {
                        let rd_off = (rd * 8) as i32;
                        dynasm!(ops
                            ; .arch x64
                            ; mov rax, QWORD link_pc
                            ; mov QWORD [r15 + rd_off], rax
                        );
                    }
                    emit_return(&mut ops, target_pc as u64);
                    inst_count += 1;
                    break;
                }

                // ── JALR (end block) ───────────────────────────────────────────
                Instruction::Jalr { rd, rs1, imm } => {
                    let link_pc = next_seq as i64;
                    let rs1_off = (rs1 * 8) as i32;
                    let imm32   = imm as i32;
                    // Read rs1 first, then write rd (handles rd == rs1 correctly).
                    dynasm!(ops
                        ; .arch x64
                        ; mov rcx, QWORD [r15 + rs1_off]
                    );
                    if rd != 0 {
                        let rd_off = (rd * 8) as i32;
                        dynasm!(ops
                            ; .arch x64
                            ; mov rax, QWORD link_pc
                            ; mov QWORD [r15 + rd_off], rax
                        );
                    }
                    // Compute target, clear LSB, emit epilogue manually
                    // (can't use emit_return because target is runtime-computed in rcx)
                    let mask: i32 = -2;
                    dynasm!(ops
                        ; .arch x64
                        ; add rcx, imm32
                        ; and rcx, mask     // clear LSB per spec (Unpriv §2.5)
                        ; add rsp, 8        // undo alignment pad from prologue
                        ; pop r14
                        ; pop r15
                        ; mov rax, rcx
                        ; ret
                    );
                    inst_count += 1;
                    break;
                }

                // FENCE / FENCE.I: no-op in our single-threaded emulator.
                // Inlined rather than slow-pathed so that ftrace's 39K
                // flush_icache_range() calls don't each break the JIT block.
                Instruction::Fence => {}

                // Everything else: slow path (end block)
                _ => {
                    block_exit!();
                    break;
                }
            }

            guest_pc = next_seq;
            inst_count += 1;
            if inst_count >= 128 {
                emit_return(&mut ops, guest_pc);
                break;
            }
        }

        // Only cache blocks that compiled at least one instruction. Blocks that
        // immediately slow-path (inst_count == 0) are not worth caching — kernel
        // VAs after paging is enabled always fail bus.load() and would flood the
        // HashMap with do-nothing stubs, adding lookup overhead for every step.
        if inst_count > 0 {
            let buf = ops.finalize().unwrap();
            let fn_ptr: JitFn = unsafe { std::mem::transmute(buf.ptr(entry)) };
            self.blocks.insert(start_pc, (buf, fn_ptr));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bus::Bus, cpu::Cpu};
    use dynasmrt::{dynasm, DynasmApi, x64::Assembler};

    fn make_cpu() -> Cpu {
        Cpu::new(Bus::new(4096, 0x8000_0000), 0x8000_0000, false)
    }

    #[test]
    fn jit_cache_new_is_empty() {
        let jit = JitCache::new();
        assert!(jit.get(0x8000_0000).is_none());
    }

    #[test]
    fn jit_cache_invalidate_clears_all() {
        let mut jit = JitCache::new();
        // manually insert a dummy block to test invalidate
        let mut ops = Assembler::new().unwrap();
        let off = ops.offset();
        dynasm!(ops ; .arch x64 ; mov rax, QWORD 42i64 ; ret);
        let buf = ops.finalize().unwrap();
        let f: JitFn = unsafe { std::mem::transmute(buf.ptr(off)) };
        jit.blocks.insert(0x8000_0000, (buf, f));
        assert!(jit.get(0x8000_0000).is_some());
        jit.invalidate();
        assert!(jit.get(0x8000_0000).is_none());
    }

    #[test]
    fn callout_store_then_load_roundtrip() {
        let mut cpu = make_cpu();
        cpu.regs[1] = 0xDEAD_BEEF_0000_0001;
        let addr = 0x8000_0010u64;

        let store_result = unsafe { jit_store64(&mut cpu as *mut Cpu, addr, cpu.regs[1]) };
        assert_eq!(store_result, 0, "store64 should return 0 on success");

        let loaded = unsafe { jit_load64(&mut cpu as *mut Cpu, addr) };
        assert_eq!(loaded, 0xDEAD_BEEF_0000_0001);
    }

    #[test]
    fn callout_load_fault_returns_sentinel() {
        let mut cpu = make_cpu();
        // Address 0x0 is outside RAM — should return u64::MAX
        let result = unsafe { jit_load64(&mut cpu as *mut Cpu, 0x0000_0000) };
        assert_eq!(result, u64::MAX);
    }

    #[test]
    fn callout_store_fault_returns_sentinel() {
        let mut cpu = make_cpu();
        let result = unsafe { jit_store64(&mut cpu as *mut Cpu, 0x0000_0000, 42) };
        assert_eq!(result, u64::MAX);
    }

    // ADDI x1, x0, 42 = 0x02A00093
    // ECALL            = 0x00000073  (slow path sentinel)
    #[test]
    fn jit_addi() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.bus.store(ram,     4, 0x02A00093u64).unwrap();
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap();

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).expect("block must be compiled");
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(cpu.regs[1], 42, "x1 should be 42 after ADDI");
        // ECALL follows a handled instruction: block ends cleanly at ECALL's addr.
        // The main loop calls cpu.step() from there to handle the slow-path opcode.
        assert_eq!(next_pc, ram + 4, "block should return ECALL addr for main-loop dispatch");
    }

    // ADD x3, x1, x2 = 0x002081B3
    #[test]
    fn jit_add() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 10;
        cpu.regs[2] = 32;
        cpu.bus.store(ram,     4, 0x002081B3u64).unwrap();
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap();

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(cpu.regs[3], 42);
        assert_eq!(next_pc, ram + 4, "block should return ECALL addr for main-loop dispatch");
    }

    // LUI x1, 1 = 0x000010B7  →  x1 = 0x0000_1000
    #[test]
    fn jit_lui() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.bus.store(ram,     4, 0x000010B7u64).unwrap();
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap();

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(cpu.regs[1], 0x0000_1000);
        assert_eq!(next_pc, ram + 4, "block should return ECALL addr for main-loop dispatch");
    }

    // MUL x3, x1, x2  = funct7=1, rs2=2, rs1=1, funct3=0, rd=3, opcode=0x33
    // = (1<<25)|(2<<20)|(1<<15)|(0<<12)|(3<<7)|0x33 = 0x022081B3
    #[test]
    fn jit_mul() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 6;
        cpu.regs[2] = 7;
        cpu.bus.store(ram,     4, 0x022081B3u64).unwrap();
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap();
        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);
        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], 42);
    }

    // DIV x3, x1, x2  = funct7=1, rs2=2, rs1=1, funct3=4, rd=3, opcode=0x33
    // = (1<<25)|(2<<20)|(1<<15)|(4<<12)|(3<<7)|0x33 = 0x0220C1B3
    #[test]
    fn jit_div() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 84;
        cpu.regs[2] = 2;
        cpu.bus.store(ram,     4, 0x0220C1B3u64).unwrap();
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap();
        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);
        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], 42);
    }

    // DIV x3, x1, x2 with x2 == 0.
    // Per RISC-V spec §7.2, signed division by zero must return -1 (u64::MAX).
    #[test]
    fn jit_div_by_zero() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 42;
        cpu.regs[2] = 0;
        cpu.bus.store(ram,     4, 0x0220C1B3u64).unwrap(); // DIV x3, x1, x2
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL (ends block)

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], u64::MAX);
    }

    // DIV x3, x1, x2 with x1 == i64::MIN, x2 == -1.
    // Per RISC-V spec §7.2, signed overflow must return i64::MIN unchanged.
    #[test]
    fn jit_div_overflow() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = i64::MIN as u64;
        cpu.regs[2] = (-1i64) as u64;
        cpu.bus.store(ram,     4, 0x0220C1B3u64).unwrap(); // DIV x3, x1, x2
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], i64::MIN as u64);
    }

    // REM x3, x1, x2  = funct7=1, rs2=2, rs1=1, funct3=6, rd=3, opcode=0x33
    // = (1<<25)|(2<<20)|(1<<15)|(6<<12)|(3<<7)|0x33 = 0x0220E1B3
    // Per RISC-V spec §7.2, remainder by zero must return the dividend (x1).
    #[test]
    fn jit_rem_div_by_zero() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 99;
        cpu.regs[2] = 0;
        cpu.bus.store(ram,     4, 0x0220E1B3u64).unwrap(); // REM x3, x1, x2
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], 99);
    }

    // SW x1, 0(x2)  encoding: funct7_imm=0, rs2=1, rs1=2, funct3=2, imm_lo=0, opcode=0x23
    // S-type: imm[11:5]=0000000, rs2=00001, rs1=00010, funct3=010, imm[4:0]=00000, opcode=0100011
    // = 0x00112023
    // LW x3, 0(x2)  encoding: imm=0, rs1=2, funct3=2, rd=3, opcode=0x03
    // = (0<<20)|(2<<15)|(2<<12)|(3<<7)|0x03 = 0x00012183
    #[test]
    fn jit_load_store() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 0xDEAD_BEEF;
        cpu.regs[2] = ram + 0x100;      // store target address in x2

        cpu.bus.store(ram,      4, 0x00112023u64).unwrap(); // SW x1, 0(x2)
        cpu.bus.store(ram + 4,  4, 0x00012183u64).unwrap(); // LW x3, 0(x2)
        cpu.bus.store(ram + 8,  4, 0x00000073u64).unwrap(); // ECALL (slow path)

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        // LW sign-extends 32 bits; DEAD_BEEF = 0xDEAD_BEEF which sign-extended is 0xFFFF_FFFF_DEAD_BEEF
        assert_eq!(cpu.regs[3], 0xFFFF_FFFF_DEAD_BEEFu64);
        assert_eq!(next_pc, ram + 8, "block should return ECALL addr for main-loop dispatch");
    }

    // LW x2, 0(x1) encoding: imm=0, rs1=1, funct3=2, rd=2, opcode=0x03
    // = (0<<20)|(1<<15)|(2<<12)|(2<<7)|0x03 = 0x0000A103
    #[test]
    fn jit_load_fault_returns_sentinel() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 0x0000_1000u64; // unmapped address — outside the 4 KiB test RAM
        cpu.bus.store(ram, 4, 0x0000A103u64).unwrap(); // LW x2, 0(x1)
        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);
        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(next_pc, u64::MAX, "faulting load must return slow-path sentinel");
    }

    #[test]
    fn jit_branch_taken() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 99;  // x1 != 0 → BNE taken
        cpu.bus.store(ram, 4, 0x00009463u64).unwrap(); // BNE x1, x0, +8

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(next_pc, ram + 8, "BNE should jump to pc+8 when x1 != 0");
    }

    #[test]
    fn jit_branch_not_taken() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 0;   // x1 == 0 → BNE not taken
        cpu.bus.store(ram, 4, 0x00009463u64).unwrap(); // BNE x1, x0, +8

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(next_pc, ram + 4, "BNE should fall through to pc+4 when x1 == 0");
    }

    #[test]
    fn jit_jal() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.bus.store(ram, 4, 0x008000EFu64).unwrap(); // JAL x1, +8

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(next_pc, ram + 8, "JAL target should be pc+8");
        assert_eq!(cpu.regs[1], ram + 4, "JAL link register should be pc+4");
    }

    // ── RVC tests ───────────────────────────────────────────────────────────
    //
    // C.ADDI x1, x1, +1  (Q1 funct3=000, rd=1, imm=1)
    // Encoding: funct3=000|bit12=0|rd=1|nzimm[4:0]=00001|01 = 0x0085
    #[test]
    fn rvc_caddi() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 41;
        cpu.bus.store(ram,   2, 0x0085u64).unwrap(); // C.ADDI x1, x1, 1
        cpu.bus.store(ram+2, 4, 0x00000073u64).unwrap(); // ECALL (slow-path sentinel)

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).expect("RVC block must be compiled");
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[1], 42, "C.ADDI x1,x1,1 must produce 42");
        assert_eq!(next_pc, ram + 2, "block must end at ECALL address");
    }

    // C.LI x1, -1 — negative immediate must sign-extend to 0xFFFFFFFFFFFFFFFF
    // Encoding: funct3=010|bit12=1|rd=1|imm[4:0]=11111|01 = 0x50FD
    #[test]
    fn rvc_cli_negative() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 0;
        cpu.bus.store(ram,   2, 0x50FDu64).unwrap(); // C.LI x1, -1
        cpu.bus.store(ram+2, 4, 0x00000073u64).unwrap();

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).expect("C.LI block must be compiled");
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[1], u64::MAX, "C.LI x1,-1 must produce 0xFFFF...FFFF (sign-extended)");
    }

    // C.MV x1, x2  (Q2 funct3=100, bit12=0, rd=1, rs2=2)
    // Encoding: 100|0|00001|00010|10 = 0x808A
    #[test]
    fn rvc_cmv() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 0;
        cpu.regs[2] = 42;
        cpu.bus.store(ram,   2, 0x808Au64).unwrap(); // C.MV x1, x2
        cpu.bus.store(ram+2, 4, 0x00000073u64).unwrap();

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[1], 42, "C.MV x1,x2 must copy regs[2] to regs[1]");
        assert_eq!(next_pc, ram + 2);
    }

    // C.BEQZ x8, +4  (Q1 funct3=110, rs1'=0→x8, imm[2:1]=10→offset 4)
    // Encoding: 110|0|00|000|00|10|0|01 = 0xC011
    #[test]
    fn rvc_cbeqz_taken() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[8] = 0; // x8 == 0 → branch taken
        cpu.bus.store(ram,   2, 0xC011u64).unwrap(); // C.BEQZ x8, +4
        cpu.bus.store(ram+2, 4, 0x00000073u64).unwrap(); // ECALL at fall-through
        cpu.bus.store(ram+4, 4, 0x00000013u64).unwrap(); // NOP at target

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(next_pc, ram + 4, "C.BEQZ taken → pc+4");
    }

    #[test]
    fn rvc_cbeqz_not_taken() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[8] = 1; // x8 != 0 → not taken
        cpu.bus.store(ram,   2, 0xC011u64).unwrap(); // C.BEQZ x8, +4
        cpu.bus.store(ram+2, 4, 0x00000073u64).unwrap();
        cpu.bus.store(ram+4, 4, 0x00000013u64).unwrap();

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(next_pc, ram + 2, "C.BEQZ not taken → fall-through");
    }

    // C.SDSP sd x1, 8(x2)  then  C.LDSP ld x3, 8(x2) — memory round-trip
    // C.SDSP: Q2 funct3=111, uimm[5:3]=001, uimm[8:6]=000, rs2=1 → 0xE406
    // C.LDSP: Q2 funct3=011, bit12=0, rd=3, uimm[4:3]=01, uimm[8:6]=000 → 0x60E2
    #[test]
    fn rvc_ldsp_sdsp_roundtrip() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 0xCAFE_BABE_1234_5678;
        cpu.regs[2] = ram + 0x100; // sp
        cpu.regs[3] = 0;

        // C.SDSP sd x1, 8(x2)
        // uimm[5:3]=bits[12:10]=001, uimm[8:6]=bits[9:7]=000, rs2=bits[6:2]=00001
        // raw16 = (111<<13)|(001<<10)|(000<<7)|(00001<<2)|10 = 0xE406
        cpu.bus.store(ram,   2, 0xE406u64).unwrap();
        // C.LDSP ld x3, 8(x2)
        // rd=3(00011), bit12=uimm[5]=0, bits[6:5]=uimm[4:3]=01, bits[4:2]=uimm[8:6]=000
        // raw16 = (011<<13)|(0<<12)|(00011<<7)|(0<<6)|(1<<5)|(000<<2)|10
        //       = 24576+0+384+0+32+0+2 = 24994 = 0x61A2
        cpu.bus.store(ram+2, 2, 0x61A2u64).unwrap();
        cpu.bus.store(ram+4, 4, 0x00000073u64).unwrap(); // ECALL

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], 0xCAFE_BABE_1234_5678, "C.LDSP must load value written by C.SDSP");
        assert_eq!(next_pc, ram + 4);
    }

    #[test]
    fn jit_slow_path() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.bus.store(ram, 4, 0xF14020F3u64).unwrap(); // CSRR x1, mhartid

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        // Blocks whose first instruction is unhandled compile to nothing and are
        // not cached. The main loop's None branch handles them via cpu.step().
        assert!(jit.get(ram).is_none(), "pure slow-path block must not be cached");
    }
}
