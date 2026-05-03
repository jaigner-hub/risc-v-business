use std::collections::HashMap;
use dynasmrt::{dynasm, DynasmApi, AssemblyOffset, ExecutableBuffer, x64::Assembler};
use crate::cpu::decode::{decode, decode_rvc, Instruction};
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
    cpu.bus.load(addr, 1).unwrap_or(u64::MAX)
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
    cpu.bus.load(addr, 2).unwrap_or(u64::MAX)
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
    cpu.bus.load(addr, 4).unwrap_or(u64::MAX)
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
    cpu.bus.load(addr, 8).unwrap_or(u64::MAX)
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
    match cpu.bus.store(addr, 1, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
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
    match cpu.bus.store(addr, 2, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
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
    match cpu.bus.store(addr, 4, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
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
    match cpu.bus.store(addr, 8, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
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

// ─── Block emitters ──────────────────────────────────────────────────────────
//
// Calling convention:
//   Entry: rdi = &mut cpu.regs[0], rsi = *mut Cpu
//   Inside block: r15 = regs base, r14 = cpu ptr (both callee-saved)
//   Return: rax = next guest PC, or u64::MAX on slow path

fn emit_prologue(ops: &mut Assembler) {
    dynasm!(ops
        ; .arch x64
        ; push r15
        ; push r14
        ; mov r15, rdi
        ; mov r14, rsi
    );
}

fn emit_return(ops: &mut Assembler, next_pc: u64) {
    let pc_val = next_pc as i64;
    dynasm!(ops
        ; .arch x64
        ; pop r14
        ; pop r15
        ; mov rax, QWORD pc_val
        ; ret
    );
}

fn emit_slow_path(ops: &mut Assembler) {
    dynasm!(ops
        ; .arch x64
        ; pop r14
        ; pop r15
        ; mov rax, QWORD -1i64
        ; ret
    );
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

    /// Compile the basic block starting at guest virtual address `start_pc`.
    /// No-op if the block is already cached. The block ends at the first
    /// unhandled instruction (slow-path return), or after 64 instructions
    /// (fall-through return to next sequential PC).
    pub fn compile(&mut self, cpu: &mut Cpu, start_pc: u64) {
        if self.blocks.contains_key(&start_pc) { return; }

        let mut ops = Assembler::new().unwrap();
        let entry: AssemblyOffset = ops.offset();
        emit_prologue(&mut ops);

        let mut guest_pc = start_pc;
        let mut inst_count = 0u32;

        loop {
            // Fetch 4 bytes. RVC blocks start with the low 2 bytes having bits[1:0] != 0b11.
            let raw4 = match cpu.bus.load(guest_pc, 4) {
                Ok(v)  => v as u32,
                Err(_) => { emit_slow_path(&mut ops); break; }
            };

            let (inst, inst_size): (Instruction, u64) = if raw4 & 0x3 != 0x3 {
                // RVC (16-bit): send to slow path immediately
                match decode_rvc(raw4 as u16) {
                    Ok(i)  => (i, 2),
                    Err(_) => { emit_slow_path(&mut ops); break; }
                }
            } else {
                match decode(raw4) {
                    Ok(i)  => (i, 4),
                    Err(_) => { emit_slow_path(&mut ops); break; }
                }
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

                // Everything else: slow path (end block)
                _ => {
                    emit_slow_path(&mut ops);
                    break;
                }
            }

            guest_pc = next_seq;
            inst_count += 1;
            if inst_count >= 64 {
                emit_return(&mut ops, guest_pc);
                break;
            }
        }

        let buf = ops.finalize().unwrap();
        let fn_ptr: JitFn = unsafe { std::mem::transmute(buf.ptr(entry)) };
        self.blocks.insert(start_pc, (buf, fn_ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bus::Bus, cpu::Cpu};
    use dynasmrt::{dynasm, DynasmApi, x64::Assembler};

    fn make_cpu() -> Cpu {
        Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false)
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
        assert_eq!(next_pc, u64::MAX, "ECALL should trigger slow path");
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
        assert_eq!(next_pc, u64::MAX);
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
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(cpu.regs[1], 0x0000_1000);
    }
}
