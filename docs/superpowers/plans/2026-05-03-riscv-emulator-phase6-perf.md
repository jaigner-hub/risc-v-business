# Phase 6a: x86-64 JIT + Peripheral Batching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the per-instruction interpret loop with a basic-block x86-64 JIT and gate peripheral polling to every 1024 blocks, achieving meaningfully faster boot without breaking any existing tests.

**Architecture:** `src/jit.rs` holds a `JitCache` (HashMap of compiled blocks) that emits real x86-64 via `dynasmrt`. `src/main.rs` is updated to dispatch through the JIT and check peripherals every 1024 ticks instead of every instruction. The interpreter remains intact and handles all slow-path exits (CSR, FP, atomics, RVC, ECALL).

**Tech Stack:** Rust 2021, `dynasmrt = "2.0"` (includes `dynasm!` macro), x86-64 SysV calling convention.

---

## File Layout

| File | Change |
|------|--------|
| `Cargo.toml` | add `dynasmrt = "2.0"` |
| `src/cpu/mod.rs` | make `regs` field `pub`; add `pub jit_invalidate: bool` field |
| `src/cpu/execute.rs` | set `cpu.jit_invalidate = true` in `satp` write arm and `SfenceVma` arm |
| `src/jit.rs` | new — `JitCache`, block compiler, memory callout helpers, unit tests |
| `src/lib.rs` | add `pub mod jit` |
| `src/main.rs` | JIT dispatch loop + 1024-instruction peripheral gate |

---

## Task 1: Cargo + Scaffold

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/cpu/mod.rs`
- Modify: `src/lib.rs`
- Create: `src/jit.rs`

- [ ] **Step 1: Add dynasmrt to Cargo.toml**

In `Cargo.toml`, add under `[dependencies]`:
```toml
dynasmrt = "2.0"
```

- [ ] **Step 2: Make `regs` public and add `jit_invalidate` to Cpu**

In `src/cpu/mod.rs`, find the `Cpu` struct. Change `regs` from private to public and add the invalidation flag:

```rust
pub struct Cpu {
    pub regs:   [u64; 32],     // was: regs: [u64; 32]
    pub fregs: [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
    pub csr: Csr,
    pub reservation: Option<u64>,
    pub mode: PrivMode,
    pub mmu: mmu::Mmu,
    pub inst_size: u64,
    pub fcsr: u32,
    pub jit_invalidate: bool,   // new field
}
```

In `Cpu::new()`, add `jit_invalidate: false` to the struct literal:
```rust
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
```

The existing `reg()` and `set_reg()` accessors still enforce x0=0; the JIT will do the same manually by skipping writes when `rd == 0`.

- [ ] **Step 3: Register the jit module in lib.rs**

In `src/lib.rs`, add:
```rust
pub mod jit;
```

Full file after change:
```rust
pub mod bus;
pub mod clint;
pub mod cpu;
pub mod dtb;
pub mod jit;
pub mod loader;
pub mod plic;
pub mod uart;
```

- [ ] **Step 4: Write the failing scaffold test**

Create `src/jit.rs` with this skeleton (tests will fail until compile() is implemented):

```rust
use std::collections::HashMap;
use dynasmrt::{dynasm, DynasmApi, DynasmLabelApi, AssemblyOffset, ExecutableBuffer, x64::Assembler};
use crate::cpu::Cpu;

/// Signature of every compiled basic block.
/// - `regs`: pointer to `cpu.regs[0]` — the 32-element u64 register file.
/// - `cpu`: opaque pointer passed through to memory callout helpers.
/// Returns: next guest PC, or `u64::MAX` for slow-path (trap / unhandled instruction).
pub type JitFn = unsafe extern "sysv64" fn(regs: *mut u64, cpu: *mut Cpu) -> u64;

pub struct JitCache {
    blocks: HashMap<u64, (ExecutableBuffer, JitFn)>,
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

    /// Compile the basic block starting at guest virtual address `pc`.
    /// No-op if the block is already cached or if instruction fetch fails.
    pub fn compile(&mut self, cpu: &mut Cpu, pc: u64) {
        if self.blocks.contains_key(&pc) { return; }
        // Implemented in Tasks 3–6.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bus::Bus, cpu::Cpu};

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
        let mut cpu = make_cpu();
        // compile() is a no-op for now, so we manually insert a dummy block to test invalidate
        let mut ops = Assembler::new().unwrap();
        let off = ops.offset();
        dynasm!(ops ; .arch x64 ; mov rax, 42i64 ; ret);
        let buf = ops.finalize().unwrap();
        let f: JitFn = unsafe { std::mem::transmute(buf.ptr(off)) };
        jit.blocks.insert(0x8000_0000, (buf, f));
        assert!(jit.get(0x8000_0000).is_some());
        jit.invalidate();
        assert!(jit.get(0x8000_0000).is_none());
    }
}
```

- [ ] **Step 5: Run the failing tests to confirm setup is correct**

```bash
cargo test jit_cache
```

Expected: both tests PASS (the scaffold is enough for them to pass — no JIT execution yet).

- [ ] **Step 6: Run all existing tests to confirm nothing is broken**

```bash
cargo test
```

Expected: all 252 tests pass. (Making `regs` public and adding `jit_invalidate` are additive changes.)

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/cpu/mod.rs src/lib.rs src/jit.rs
git commit -m "feat(jit): scaffold JitCache, make regs pub, add jit_invalidate flag"
```

---

## Task 2: Memory Callout Helpers

**Files:**
- Modify: `src/jit.rs`

The JIT calls these helpers for all loads and stores. Each is `extern "sysv64"` so the JIT can call them with a plain `call rax` after setting up the SysV args.

- [ ] **Step 1: Write failing tests for the callout helpers**

Add to the `#[cfg(test)]` block in `src/jit.rs`:

```rust
    #[test]
    fn callout_store_then_load_roundtrip() {
        let mut cpu = make_cpu();
        cpu.regs[1] = 0xDEAD_BEEF_0000_0001;
        let addr = 0x8000_0010u64;

        // Store 64-bit value, then load it back
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
```

- [ ] **Step 2: Run to confirm tests fail**

```bash
cargo test callout
```

Expected: compile error — `jit_load64`, `jit_store64` not defined yet.

- [ ] **Step 3: Implement the callout helpers**

Add these functions to `src/jit.rs`, above the `impl JitCache` block:

```rust
/// Load 1 byte (zero-extended to u64). Returns u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load8(cpu: *mut Cpu, va: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.load(va, 1) {
        Ok(v)  => v,
        Err(_) => u64::MAX,
    }
}

/// Load 2 bytes (zero-extended to u64). Returns u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load16(cpu: *mut Cpu, va: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.load(va, 2) {
        Ok(v)  => v,
        Err(_) => u64::MAX,
    }
}

/// Load 4 bytes (zero-extended to u64). Returns u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load32(cpu: *mut Cpu, va: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.load(va, 4) {
        Ok(v)  => v,
        Err(_) => u64::MAX,
    }
}

/// Load 8 bytes. Returns u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load64(cpu: *mut Cpu, va: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.load(va, 8) {
        Ok(v)  => v,
        Err(_) => u64::MAX,
    }
}

/// Store 1 byte. Returns 0 on success, u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store8(cpu: *mut Cpu, va: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(va, 1, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}

/// Store 2 bytes. Returns 0 on success, u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store16(cpu: *mut Cpu, va: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(va, 2, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}

/// Store 4 bytes. Returns 0 on success, u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store32(cpu: *mut Cpu, va: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(va, 4, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}

/// Store 8 bytes. Returns 0 on success, u64::MAX on fault.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store64(cpu: *mut Cpu, va: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(va, 8, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}
```

**Note:** Phase 6a uses physical addresses in the bus (no MMU translation in the callouts). This matches the approach of compile-time block caching: when a block is compiled, its PC is already mapped. Full MMU-aware callouts are a Phase 6b concern.

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test callout
```

Expected: all 3 callout tests PASS.

- [ ] **Step 5: Run all tests**

```bash
cargo test
```

Expected: all 252 tests + 2 scaffold tests + 3 callout tests = 257 total, all passing.

- [ ] **Step 6: Commit**

```bash
git add src/jit.rs
git commit -m "feat(jit): add extern sysv64 memory callout helpers (load/store 8/16/32/64)"
```

---

## Task 3: Integer Arithmetic Block Compiler

**Files:**
- Modify: `src/jit.rs`

Implements `compile()` for: RV64I R-type (ADD/SUB/AND/OR/XOR/SLL/SRL/SRA/SLT/SLTU), I-type arithmetic (ADDI/ANDI/ORI/XORI/SLLI/SRLI/SRAI/SLTI/SLTIU), W-variants (ADDW/SUBW/SLLW/SRLW/SRAW/ADDIW/SLLIW/SRLIW/SRAIW), and upper immediates (LUI/AUIPC).

The compile loop structure established here is extended in Tasks 4–6.

- [ ] **Step 1: Write the failing unit test**

Add to `src/jit.rs` tests block:

```rust
    // ADDI x1, x0, 42  encoding: imm=42, rs1=0, funct3=0, rd=1, opcode=0x13
    // = (42 << 20) | (0 << 15) | (0 << 12) | (1 << 7) | 0x13 = 0x02A00093
    // ECALL = 0x00000073  (slow-path sentinel — ends the block)
    #[test]
    fn jit_addi() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.bus.store(ram,     4, 0x02A00093u64).unwrap(); // ADDI x1, x0, 42
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL  -> slow path

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).expect("block must be compiled");
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(cpu.regs[1], 42, "x1 should be 42 after ADDI");
        assert_eq!(next_pc, u64::MAX, "ECALL should trigger slow path");
    }

    // ADD x3, x1, x2  encoding: funct7=0, rs2=2, rs1=1, funct3=0, rd=3, opcode=0x33
    // = (0<<25)|(2<<20)|(1<<15)|(0<<12)|(3<<7)|0x33 = 0x002081B3
    #[test]
    fn jit_add() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 10;
        cpu.regs[2] = 32;
        cpu.bus.store(ram,     4, 0x002081B3u64).unwrap(); // ADD x3, x1, x2
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(cpu.regs[3], 42);
        assert_eq!(next_pc, u64::MAX);
    }

    // LUI x1, 1  encoding: imm_upper=1, rd=1, opcode=0x37
    // = (1<<12)|(1<<7)|0x37 = 0x000010B7  — rd = 0x0000_1000
    #[test]
    fn jit_lui() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.bus.store(ram,     4, 0x000010B7u64).unwrap(); // LUI x1, 1
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };

        assert_eq!(cpu.regs[1], 0x0000_1000);
    }
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test jit_addi jit_add jit_lui
```

Expected: all 3 fail — compile() is currently a no-op so `jit.get(ram)` returns `None`.

- [ ] **Step 3: Add decode imports and the emit helper at the top of jit.rs**

Add these imports at the top of `src/jit.rs`:

```rust
use crate::cpu::decode::{decode, decode_rvc, Instruction};
```

Add these private helpers just below the imports, before the callout functions:

```rust
/// Emit the standard block prologue into `ops`.
/// Sets r15 = regs base (rdi), r14 = cpu ptr (rsi). Both are callee-saved.
fn emit_prologue(ops: &mut Assembler) {
    dynasm!(ops
        ; .arch x64
        ; push r15
        ; push r14
        ; mov r15, rdi
        ; mov r14, rsi
    );
}

/// Emit epilogue + unconditional return of `next_pc`.
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

/// Emit epilogue + slow-path return (u64::MAX = -1i64).
fn emit_slow_path(ops: &mut Assembler) {
    dynasm!(ops
        ; .arch x64
        ; pop r14
        ; pop r15
        ; mov rax, -1i64
        ; ret
    );
}
```

- [ ] **Step 4: Implement compile() with integer arithmetic support**

Replace the stub `compile()` body with this full implementation. The loop decodes instructions one at a time. Branches/jumps/CSR end the block; unrecognized instructions emit a slow-path return.

```rust
pub fn compile(&mut self, cpu: &mut Cpu, start_pc: u64) {
    if self.blocks.contains_key(&start_pc) { return; }

    let mut ops = Assembler::new().unwrap();
    let entry: AssemblyOffset = ops.offset();
    emit_prologue(&mut ops);

    let mut guest_pc = start_pc;
    let mut inst_count = 0u32;

    loop {
        // Fetch instruction bytes from RAM (physical — no MMU translation in JIT).
        let raw4 = match cpu.bus.load(guest_pc, 4) {
            Ok(v)  => v as u32,
            Err(_) => { emit_slow_path(&mut ops); break; }
        };

        // Detect RVC (16-bit) vs full 32-bit.
        let (inst, inst_size): (Instruction, u64) = if raw4 & 0x3 != 0x3 {
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

        let next_seq = guest_pc.wrapping_add(inst_size); // sequential next PC

        match inst {
            // ── R-type integer ─────────────────────────────────────────────
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
                // SLL uses cl (low byte of rcx) as shift amount, masked to 6 bits.
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }

            // ── I-type arithmetic ──────────────────────────────────────────
            Instruction::Addi { rd, rs1, imm } => {
                let rs1_off = (rs1 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                let imm32   = imm as i32;
                dynasm!(ops
                    ; .arch x64
                    ; mov rax, QWORD [r15 + rs1_off]
                    ; add rax, imm32
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
            Instruction::Slli { rd, rs1, shamt } => {
                let rs1_off = (rs1 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                let sh      = shamt as i8;
                dynasm!(ops
                    ; .arch x64
                    ; mov rax, QWORD [r15 + rs1_off]
                    ; shl rax, sh
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }
            Instruction::Sltiu { rd, rs1, imm } => {
                let rs1_off = (rs1 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                let imm32   = imm as i32; // sign-extended, then compared unsigned
                dynasm!(ops
                    ; .arch x64
                    ; mov rax, QWORD [r15 + rs1_off]
                    ; cmp rax, imm32
                    ; setb al
                    ; movzx rax, al
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }

            // ── W-variants (32-bit ops, result sign-extended to 64) ────────
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }

            // ── Upper immediates ───────────────────────────────────────────
            Instruction::Lui { rd, imm } => {
                // imm is already the full 32-bit sign-extended value (upper 20 bits << 12).
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
                    let rd_off  = (rd * 8) as i32;
                    let result  = (guest_pc as i64).wrapping_add(imm as i64);
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD result
                        ; mov QWORD [r15 + rd_off], rax
                    );
                }
            }

            // ── All other instructions: slow path, end block ───────────────
            _ => {
                emit_slow_path(&mut ops);
                break;
            }
        }

        guest_pc = next_seq;
        inst_count += 1;

        // Block limit: fall through with sequential PC.
        if inst_count >= 64 {
            emit_return(&mut ops, guest_pc);
            break;
        }
    }

    let buf = ops.finalize().unwrap();
    let fn_ptr: JitFn = unsafe { std::mem::transmute(buf.ptr(entry)) };
    self.blocks.insert(start_pc, (buf, fn_ptr));
}
```

- [ ] **Step 5: Add the emit helper functions used above**

Add these private helpers below `emit_slow_path` (before the callout functions):

```rust
/// Load rs1→rax, rs2→rcx, apply `op`, optionally write rax→rd.
fn emit_r_op<F>(ops: &mut Assembler, rd: usize, rs1: usize, rs2: usize, op: F)
where
    F: FnOnce(&mut Assembler),
{
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

/// Load rs1→rax, apply `op` with sign-extended `imm`, optionally write rax→rd.
fn emit_i_op<F>(ops: &mut Assembler, rd: usize, rs1: usize, imm: i64, op: F)
where
    F: FnOnce(&mut Assembler, i32),
{
    let rs1_off = (rs1 * 8) as i32;
    let rd_off  = (rd  * 8) as i32;
    let imm32   = imm as i32;
    dynasm!(ops ; .arch x64 ; mov rax, QWORD [r15 + rs1_off]);
    op(ops, imm32);
    if rd != 0 {
        dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
    }
}

/// Load lower 32 bits of rs1→eax, rs2→ecx, apply 32-bit `op`, sign-extend, optionally write rd.
fn emit_w_op<F>(ops: &mut Assembler, rd: usize, rs1: usize, rs2: usize, op: F)
where
    F: FnOnce(&mut Assembler),
{
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
```

- [ ] **Step 6: Run tests to confirm they pass**

```bash
cargo test jit_addi jit_add jit_lui
```

Expected: all 3 PASS.

- [ ] **Step 7: Run all tests**

```bash
cargo test
```

Expected: all passing (252 + JIT tests).

- [ ] **Step 8: Commit**

```bash
git add src/jit.rs
git commit -m "feat(jit): compile integer arithmetic (R-type, I-type, W-variants, LUI/AUIPC)"
```

---

## Task 4: M-Extension Block Compiler

**Files:**
- Modify: `src/jit.rs`

Adds MUL/MULH/MULHSU/MULHU/MULW and DIV/DIVU/DIVW/DIVUW/REM/REMU/REMW/REMUW to the compile() match block.

- [ ] **Step 1: Write failing tests**

Add to `src/jit.rs` tests:

```rust
    // MUL x3, x1, x2  encoding: funct7=1, rs2=2, rs1=1, funct3=0, rd=3, opcode=0x33
    // = (1<<25)|(2<<20)|(1<<15)|(0<<12)|(3<<7)|0x33 = 0x022081B3
    #[test]
    fn jit_mul() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 6;
        cpu.regs[2] = 7;
        cpu.bus.store(ram,     4, 0x022081B3u64).unwrap(); // MUL x3, x1, x2
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], 42);
    }

    // DIV x3, x1, x2  encoding: funct7=1, rs2=2, rs1=1, funct3=4, rd=3, opcode=0x33
    // = (1<<25)|(2<<20)|(1<<15)|(4<<12)|(3<<7)|0x33 = 0x0220C1B3
    #[test]
    fn jit_div() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.regs[1] = 84;
        cpu.regs[2] = 2;
        cpu.bus.store(ram,     4, 0x0220C1B3u64).unwrap(); // DIV x3, x1, x2
        cpu.bus.store(ram + 4, 4, 0x00000073u64).unwrap(); // ECALL

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(cpu.regs[3], 42);
    }
```

- [ ] **Step 2: Run to confirm they fail**

```bash
cargo test jit_mul jit_div
```

Expected: FAIL — MUL/DIV currently fall through to `_` → slow path → `u64::MAX` returned, x3 not written.

- [ ] **Step 3: Add M-extension arms to the compile() match block**

Insert these match arms in `compile()` before the `_ =>` fallthrough. DIV/REM require inline division-by-zero and overflow guards following the RISC-V spec (Unpriv §7.2):

```rust
            // ── M-extension multiplies ─────────────────────────────────────
            Instruction::Mul { rd, rs1, rs2 } => {
                emit_r_op(&mut ops, rd, rs1, rs2, |ops| {
                    dynasm!(ops ; .arch x64 ; imul rax, rcx);
                });
            }
            Instruction::Mulh { rd, rs1, rs2 } => {
                // MULH: signed × signed, upper 64 bits. Result in rdx after imul.
                let rs1_off = (rs1 * 8) as i32;
                let rs2_off = (rs2 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                dynasm!(ops
                    ; .arch x64
                    ; mov rax, QWORD [r15 + rs1_off]
                    ; mov rcx, QWORD [r15 + rs2_off]
                    ; imul rcx          // rdx:rax = rax * rcx (signed)
                    ; mov rax, rdx
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }
            Instruction::Mulhu { rd, rs1, rs2 } => {
                // MULHU: unsigned × unsigned, upper 64 bits.
                let rs1_off = (rs1 * 8) as i32;
                let rs2_off = (rs2 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                dynasm!(ops
                    ; .arch x64
                    ; mov rax, QWORD [r15 + rs1_off]
                    ; mov rcx, QWORD [r15 + rs2_off]
                    ; mul rcx           // rdx:rax = rax * rcx (unsigned)
                    ; mov rax, rdx
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }
            Instruction::Mulhsu { rd, rs1, rs2 } => {
                // MULHSU: signed rs1 × unsigned rs2, upper 64 bits.
                // Emulate: if rs1 >= 0, same as MULHU. If rs1 < 0, negate, MULHU, negate if rs2 != 0.
                // Simpler: use 128-bit Rust arithmetic via a callout. Emit as slow path for now.
                emit_slow_path(&mut ops);
                break;
            }
            Instruction::Mulw { rd, rs1, rs2 } => {
                // MULW: 32-bit signed multiply, sign-extend result to 64 bits.
                let rs1_off = (rs1 * 8) as i32;
                let rs2_off = (rs2 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                dynasm!(ops
                    ; .arch x64
                    ; mov eax, DWORD [r15 + rs1_off]
                    ; imul eax, DWORD [r15 + rs2_off]
                    ; movsxd rax, eax
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }

            // ── M-extension divides and remainders ─────────────────────────
            Instruction::Div { rd, rs1, rs2 } => {
                let rs1_off = (rs1 * 8) as i32;
                let rs2_off = (rs2 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                let done    = ops.new_dynamic_label();
                dynasm!(ops
                    ; .arch x64
                    ; mov rax, QWORD [r15 + rs1_off]
                    ; mov rcx, QWORD [r15 + rs2_off]
                    // Division by zero: quotient = -1 (all bits set)
                    ; test rcx, rcx
                    ; jnz >not_zero
                    ; mov rax, -1i64
                    ; jmp =>done
                    ;not_zero:
                    // Overflow: i64::MIN / -1 = i64::MIN (not representable, return i64::MIN)
                    ; mov rdx, QWORD i64::MIN as i64
                    ; cmp rax, rdx
                    ; jne >no_overflow
                    ; cmp rcx, -1i32
                    ; jne >no_overflow
                    ; jmp =>done          // rax already holds i64::MIN
                    ;no_overflow:
                    ; cqo                 // sign-extend rax → rdx:rax
                    ; idiv rcx            // quotient in rax
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                    ; mov rax, -1i64    // div by zero = u64::MAX
                    ; jmp =>done
                    ;not_zero:
                    ; xor rdx, rdx      // zero-extend rax
                    ; div rcx
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                    ; test rcx, rcx
                    ; jnz >not_zero
                    ; jmp =>done          // rem of div-by-zero = dividend, already in rax
                    ;not_zero:
                    ; mov rdx, QWORD i64::MIN as i64
                    ; cmp rax, rdx
                    ; jne >no_overflow
                    ; cmp rcx, -1i32
                    ; jne >no_overflow
                    ; xor rax, rax        // overflow case: remainder = 0
                    ; jmp =>done
                    ;no_overflow:
                    ; cqo
                    ; idiv rcx
                    ; mov rax, rdx        // remainder is in rdx
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }
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
                    ; mov rax, -1i64
                    ; jmp =>done
                    ;not_zero:
                    ; mov rdx, QWORD i64::MIN as i64
                    ; cmp rax, rdx
                    ; jne >no_overflow
                    ; cmp rcx, -1i64
                    ; jne >no_overflow
                    ; jmp =>done
                    ;no_overflow:
                    ; cqo
                    ; idiv rcx
                    ; movsxd rax, eax
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }
            Instruction::Divuw { rd, rs1, rs2 } => {
                let rs1_off = (rs1 * 8) as i32;
                let rs2_off = (rs2 * 8) as i32;
                let rd_off  = (rd  * 8) as i32;
                let done    = ops.new_dynamic_label();
                dynasm!(ops
                    ; .arch x64
                    ; mov eax, DWORD [r15 + rs1_off]  // zero-extend to rax
                    ; mov ecx, DWORD [r15 + rs2_off]  // zero-extend to rcx
                    ; test ecx, ecx
                    ; jnz >not_zero
                    ; mov rax, -1i64
                    ; jmp =>done
                    ;not_zero:
                    ; xor edx, edx
                    ; div ecx
                    ; movsxd rax, eax
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                    ; movsxd rax, eax   // rem of div-by-zero = dividend, sign-extended
                    ; jmp =>done
                    ;not_zero:
                    ; mov rdx, QWORD i64::MIN as i64
                    ; cmp rax, rdx
                    ; jne >no_overflow
                    ; cmp rcx, -1i64
                    ; jne >no_overflow
                    ; xor eax, eax
                    ; jmp =>done
                    ;no_overflow:
                    ; cqo
                    ; idiv rcx
                    ; movsxd rax, edx
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
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
                    ; movsxd rax, eax
                    ; jmp =>done
                    ;not_zero:
                    ; xor edx, edx
                    ; div ecx
                    ; movsxd rax, edx
                    ; =>done:
                );
                if rd != 0 {
                    dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
                }
            }
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test jit_mul jit_div
```

Expected: PASS.

- [ ] **Step 5: Run all tests**

```bash
cargo test
```

Expected: all passing.

- [ ] **Step 6: Commit**

```bash
git add src/jit.rs
git commit -m "feat(jit): compile M-extension (MUL/DIV/REM + W-variants)"
```

---

## Task 5: Load/Store Block Compiler

**Files:**
- Modify: `src/jit.rs`

Adds LB/LH/LW/LD/LBU/LHU/LWU (loads) and SB/SH/SW/SD (stores) to the compile() match. All use the callout helpers. After every callout, the JIT checks `rax == -1` and exits slow-path on fault.

- [ ] **Step 1: Write failing test**

Add to `src/jit.rs` tests:

```rust
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
        assert_eq!(cpu.regs[3], 0xFFFF_FFFF_DEAD_BEEF_u64);
        assert_eq!(next_pc, u64::MAX);
    }
```

- [ ] **Step 2: Run to confirm the test fails**

```bash
cargo test jit_load_store
```

Expected: FAIL — loads/stores currently fall to slow path.

- [ ] **Step 3: Add a helper for load callouts**

Add this private function below `emit_w_op` in `src/jit.rs`:

```rust
/// Emit a load callout for a single instruction.
/// Sets up rdi=r14 (cpu), rsi=base+offset, calls helper, checks for fault.
/// On fault: emits slow-path epilogue and returns `true` so the caller can `break`.
/// On success: writes rax to rd (if rd != 0) and returns `false`.
/// Caller must `break` the compile loop if this returns `true`.
fn emit_load(
    ops: &mut Assembler,
    rd: usize,
    rs1: usize,
    imm: i64,
    helper: unsafe extern "sysv64" fn(*mut Cpu, u64) -> u64,
) {
    let rs1_off = (rs1 * 8) as i32;
    let rd_off  = (rd  * 8) as i32;
    let imm32   = imm as i32;
    let helper_addr = helper as i64;
    let fault_label = ops.new_dynamic_label();
    dynasm!(ops
        ; .arch x64
        ; mov rsi, QWORD [r15 + rs1_off]   // base register
        ; add rsi, imm32                    // effective address
        ; mov rdi, r14                      // cpu ptr
        ; mov rax, QWORD helper_addr
        ; call rax
        // rax == u64::MAX means fault
        ; cmp rax, -1i32
        ; je =>fault_label
    );
    if rd != 0 {
        dynasm!(ops ; .arch x64 ; mov QWORD [r15 + rd_off], rax);
    }
    // Jump past the fault handler (emitted after this call site).
    // We use a forward local jump to skip the fault epilogue.
    let skip_fault = ops.new_dynamic_label();
    dynasm!(ops ; .arch x64 ; jmp =>skip_fault ; =>fault_label:);
    emit_slow_path(ops);
    dynasm!(ops ; .arch x64 ; =>skip_fault:);
}

/// Emit a store callout. Like emit_load but takes rs2 (source register) and uses store helper.
fn emit_store(
    ops: &mut Assembler,
    rs1: usize,
    rs2: usize,
    imm: i64,
    helper: unsafe extern "sysv64" fn(*mut Cpu, u64, u64) -> u64,
) {
    let rs1_off = (rs1 * 8) as i32;
    let rs2_off = (rs2 * 8) as i32;
    let imm32   = imm as i32;
    let helper_addr = helper as i64;
    let fault_label = ops.new_dynamic_label();
    dynasm!(ops
        ; .arch x64
        ; mov rsi, QWORD [r15 + rs1_off]   // base
        ; add rsi, imm32                    // effective address
        ; mov rdx, QWORD [r15 + rs2_off]   // value to store
        ; mov rdi, r14
        ; mov rax, QWORD helper_addr
        ; call rax
        ; cmp rax, -1i32
        ; je =>fault_label
    );
    let skip_fault = ops.new_dynamic_label();
    dynasm!(ops ; .arch x64 ; jmp =>skip_fault ; =>fault_label:);
    emit_slow_path(ops);
    dynasm!(ops ; .arch x64 ; =>skip_fault:);
}
```

**Note:** After `emit_load` / `emit_store`, if a fault was taken, the slow-path `ret` has already been emitted inside the helper. Code after the `skip_fault` label is only reached when there was no fault. The compile loop can continue normally.

However, there is a subtlety: after a fault the block has emitted a `ret`, so the block ends mid-stream from the JIT's perspective. To keep the structure simple, we treat the fault path as a block exit. The load/store helpers are designed so that if the `je =>fault_label` branch is taken, the block returns `u64::MAX` and the run loop re-executes the faulting instruction via `cpu.step()`.

- [ ] **Step 4: Add load/store arms to the compile() match block**

Insert these arms before the `_ =>` fallthrough in `compile()`:

```rust
            // ── Loads (via callout helpers) ────────────────────────────────
            Instruction::Lb { rd, rs1, imm } => {
                emit_load(&mut ops, rd, rs1, imm as i64, jit_load8);
                // sign-extend byte to 64 bits
                if rd != 0 {
                    let rd_off = (rd * 8) as i32;
                    dynasm!(ops ; .arch x64 ; movsx rax, BYTE [r15 + rd_off] ; mov QWORD [r15 + rd_off], rax);
                }
            }
            Instruction::Lh { rd, rs1, imm } => {
                emit_load(&mut ops, rd, rs1, imm as i64, jit_load16);
                if rd != 0 {
                    let rd_off = (rd * 8) as i32;
                    dynasm!(ops ; .arch x64 ; movsx rax, WORD [r15 + rd_off] ; mov QWORD [r15 + rd_off], rax);
                }
            }
            Instruction::Lw { rd, rs1, imm } => {
                emit_load(&mut ops, rd, rs1, imm as i64, jit_load32);
                if rd != 0 {
                    let rd_off = (rd * 8) as i32;
                    dynasm!(ops ; .arch x64 ; movsxd rax, DWORD [r15 + rd_off] ; mov QWORD [r15 + rd_off], rax);
                }
            }
            Instruction::Ld { rd, rs1, imm } => {
                emit_load(&mut ops, rd, rs1, imm as i64, jit_load64);
            }
            Instruction::Lbu { rd, rs1, imm } => {
                emit_load(&mut ops, rd, rs1, imm as i64, jit_load8);
                // zero-extend already done by load8 (returns u64)
            }
            Instruction::Lhu { rd, rs1, imm } => {
                emit_load(&mut ops, rd, rs1, imm as i64, jit_load16);
            }
            Instruction::Lwu { rd, rs1, imm } => {
                emit_load(&mut ops, rd, rs1, imm as i64, jit_load32);
            }

            // ── Stores (via callout helpers) ───────────────────────────────
            Instruction::Sb { rs1, rs2, imm } => {
                emit_store(&mut ops, rs1, rs2, imm as i64, jit_store8);
            }
            Instruction::Sh { rs1, rs2, imm } => {
                emit_store(&mut ops, rs1, rs2, imm as i64, jit_store16);
            }
            Instruction::Sw { rs1, rs2, imm } => {
                emit_store(&mut ops, rs1, rs2, imm as i64, jit_store32);
            }
            Instruction::Sd { rs1, rs2, imm } => {
                emit_store(&mut ops, rs1, rs2, imm as i64, jit_store64);
            }
```

**Note about Lb/Lh/Lw sign extension:** `emit_load` writes the zero-extended callout result to `[r15 + rd_off]`. The sign-extension code immediately re-reads and re-writes with `movsx`/`movsxd`. This is correct but reads back what was just written. An optimization (not required here) would be to sign-extend in-register before storing.

- [ ] **Step 5: Run tests**

```bash
cargo test jit_load_store
```

Expected: PASS.

- [ ] **Step 6: Run all tests**

```bash
cargo test
```

Expected: all passing.

- [ ] **Step 7: Commit**

```bash
git add src/jit.rs
git commit -m "feat(jit): compile loads (LB/LH/LW/LD/LBU/LHU/LWU) and stores (SB/SH/SW/SD)"
```

---

## Task 6: Branch and Jump Block Compiler

**Files:**
- Modify: `src/jit.rs`

Adds BEQ/BNE/BLT/BGE/BLTU/BGEU (branches) and JAL/JALR (jumps). All end the block; branches emit a conditional return choosing between taken PC and fall-through PC.

- [ ] **Step 1: Write failing tests**

Add to `src/jit.rs` tests:

```rust
    // BNE x1, x0, +8  encoding: see plan — 0x00009463
    // With x1 != 0: should return taken PC = pc + 8
    // With x1 == 0: should return fall-through PC = pc + 4
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

    // JAL x1, +8  encoding: J-type, rd=1, imm=8, opcode=0x6F
    // imm=8: imm[20]=0, imm[19:12]=0, imm[11]=0, imm[10:1]=0b0000000100
    // J-type inst[31]=imm[20], inst[30:21]=imm[10:1], inst[20]=imm[11], inst[19:12]=imm[19:12], inst[11:7]=rd
    // = (0<<31)|(0b0000000100<<21)|(0<<20)|(0<<12)|(1<<7)|0x6F
    // = (4<<21)|(1<<7)|0x6F = 0x008000EF
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

    // CSRR x1, mhartid = CSRRS x1, 0xF14, x0 = 0xF14020F3
    #[test]
    fn jit_slow_path() {
        let ram = 0x8000_0000u64;
        let mut cpu = make_cpu();
        cpu.bus.store(ram, 4, 0xF14020F3u64).unwrap(); // CSRR x1, mhartid

        let mut jit = JitCache::new();
        jit.compile(&mut cpu, ram);

        let f = jit.get(ram).unwrap();
        let next_pc = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
        assert_eq!(next_pc, u64::MAX, "CSR instruction should trigger slow path");
    }
```

- [ ] **Step 2: Run to confirm they fail**

```bash
cargo test jit_branch_taken jit_branch_not_taken jit_jal jit_slow_path
```

Expected: `jit_branch_taken`, `jit_branch_not_taken`, `jit_jal` FAIL; `jit_slow_path` MAY already pass (CSR already falls through to `_` → slow path).

- [ ] **Step 3: Add branch and jump arms to compile()**

Insert before the `_ =>` fallthrough in `compile()`:

```rust
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
                // Emit the right conditional jump based on variant.
                match inst {
                    Instruction::Beq  { .. } => dynasm!(ops ; .arch x64 ; je  =>taken_lbl),
                    Instruction::Bne  { .. } => dynasm!(ops ; .arch x64 ; jne =>taken_lbl),
                    Instruction::Blt  { .. } => dynasm!(ops ; .arch x64 ; jl  =>taken_lbl),
                    Instruction::Bge  { .. } => dynasm!(ops ; .arch x64 ; jge =>taken_lbl),
                    Instruction::Bltu { .. } => dynasm!(ops ; .arch x64 ; jb  =>taken_lbl),
                    Instruction::Bgeu { .. } => dynasm!(ops ; .arch x64 ; jae =>taken_lbl),
                    _ => unreachable!(),
                }
                // Fall-through path:
                emit_return(&mut ops, fall_pc as u64);
                // Taken path:
                dynasm!(ops ; .arch x64 ; =>taken_lbl:);
                emit_return(&mut ops, taken_pc as u64);
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
                    ; mov rcx, QWORD [r15 + rs1_off]   // target base (save before rd write)
                );
                if rd != 0 {
                    let rd_off = (rd * 8) as i32;
                    dynasm!(ops
                        ; .arch x64
                        ; mov rax, QWORD link_pc
                        ; mov QWORD [r15 + rd_off], rax
                    );
                }
                dynasm!(ops
                    ; .arch x64
                    ; add rcx, imm32
                    ; and rcx, -2i32    // clear LSB per spec (Unpriv §2.5)
                    ; pop r14
                    ; pop r15
                    ; mov rax, rcx
                    ; ret
                );
                break;
            }
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test jit_branch_taken jit_branch_not_taken jit_jal jit_slow_path
```

Expected: all 4 PASS.

- [ ] **Step 5: Run all tests**

```bash
cargo test
```

Expected: all passing.

- [ ] **Step 6: Commit**

```bash
git add src/jit.rs
git commit -m "feat(jit): compile branches (BEQ/BNE/BLT/BGE/BLTU/BGEU) and jumps (JAL/JALR)"
```

---

## Task 7: jit_invalidate Flag in execute.rs

**Files:**
- Modify: `src/cpu/execute.rs`

Sets `cpu.jit_invalidate = true` when the guest writes to `satp` (page-table switch) or executes `SFENCE.VMA` (TLB flush). Both events require flushing the JIT block cache since virtual→physical mappings may have changed.

- [ ] **Step 1: Write a failing test**

Add to the `#[cfg(test)]` block in `src/cpu/mod.rs`:

```rust
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
        execute(&mut c, Instruction::SfenceVma { rs1: 0, rs2: 0 }).unwrap();
        assert!(c.jit_invalidate, "SFENCE.VMA must set jit_invalidate");
    }
```

- [ ] **Step 2: Run to confirm they fail**

```bash
cargo test satp_write_sets_jit_invalidate sfence_sets_jit_invalidate
```

Expected: FAIL — `jit_invalidate` is never set to `true`.

- [ ] **Step 3: Wire the flag in execute.rs**

In `src/cpu/execute.rs`, find the `SfenceVma` arm. Add `cpu.jit_invalidate = true;` to it:

```rust
Instruction::SfenceVma { .. } => {
    cpu.mmu.flush_tlb();
    cpu.jit_invalidate = true;   // add this line
    cpu.pc += cpu.inst_size;
}
```

Find the CSR write arm that handles `satp` (CSR address `0x180`). It is inside the `Csrrw` / `Csrrs` / `Csrrc` match. Look for where `csr == 0x180` is handled or where `cpu.csr_write(csr, new_val)` is called. After any write to `satp`, set the flag:

```rust
// Wherever satp (0x180) is written, e.g. in the Csrrw arm:
Instruction::Csrrw { rd, rs1, csr } => {
    let old = cpu.csr_read(csr);
    let new_val = cpu.reg(rs1);
    cpu.set_reg(rd, old);
    cpu.csr_write(csr, new_val);
    if csr == 0x180 {
        cpu.mmu.flush_tlb();
        cpu.jit_invalidate = true;
    }
    cpu.pc += cpu.inst_size;
}
```

Apply the same `if csr == 0x180` guard to `Csrrs` and `Csrrc` arms wherever they write to a CSR (only when the write is non-trivial — i.e., `rs1 != 0` for CSRRS/CSRRC since `rs1 == 0` means read-only).

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test satp_write_sets_jit_invalidate sfence_sets_jit_invalidate
```

Expected: PASS.

- [ ] **Step 5: Run all tests**

```bash
cargo test
```

Expected: all 252 + new JIT tests all passing.

- [ ] **Step 6: Commit**

```bash
git add src/cpu/mod.rs src/cpu/execute.rs
git commit -m "feat(jit): set jit_invalidate on satp write and SFENCE.VMA"
```

---

## Task 8: Update Run Loop in main.rs

**Files:**
- Modify: `src/main.rs`

Replace the current one-instruction-per-iteration loop with JIT dispatch. Peripheral polling (CLINT tick, STIP, SEIP) moves to every 1024 iterations. The `jit_invalidate` flag is checked and cleared after every step.

- [ ] **Step 1: Read the current run loop**

Read `src/main.rs` lines 97–126 to see the exact current loop body before modifying it.

- [ ] **Step 2: Replace the run loop**

Replace the entire `loop { ... }` body in `main()` with:

```rust
    use riscv_emu::jit::JitCache;
    let mut jit = JitCache::new();
    let mut tick: u64 = 0;

    loop {
        match jit.get(cpu.pc) {
            Some(f) => {
                let next = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
                if next == u64::MAX {
                    cpu.step()?;
                } else {
                    cpu.pc = next;
                }
            }
            None => {
                cpu.step()?;
                jit.compile(&mut cpu, cpu.pc);
            }
        }

        if cpu.jit_invalidate {
            cpu.jit_invalidate = false;
            jit.invalidate();
        }

        tick = tick.wrapping_add(1);
        if tick & 1023 == 0 {
            // Drain stdin → UART RX only after the shell prompt is ready.
            if cpu.bus.uart.stdin_ready {
                while let Ok(byte) = stdin_rx.try_recv() {
                    cpu.bus.uart.push_rx(byte);
                }
            }
            if cpu.bus.clint.tick() {
                cpu.csr.mip |= 1 << 7;
            } else {
                cpu.csr.mip &= !(1u64 << 7);
            }
            if cpu.bus.clint.mtime >= cpu.csr.stimecmp {
                cpu.csr.mip |= 1 << 5;
            } else {
                cpu.csr.mip &= !(1u64 << 5);
            }
            let uart_irq = cpu.bus.uart.irq_pending();
            cpu.bus.plic.set_pending(10, uart_irq);
            if cpu.bus.plic.has_interrupt() {
                cpu.csr.mip |= 1 << 9;
            } else {
                cpu.csr.mip &= !(1u64 << 9);
            }
        }
    }
```

Add the import at the top of `main.rs`:
```rust
use riscv_emu::cpu::Cpu;
```

(Required because `&mut cpu as *mut Cpu` needs the type in scope in `main.rs`. If `Cpu` is already imported via the existing `use riscv_emu::{cpu::Cpu, ...}`, no change is needed.)

- [ ] **Step 3: Build to confirm it compiles**

```bash
cargo build
```

Expected: clean build with no errors.

- [ ] **Step 4: Run all tests to confirm nothing regressed**

```bash
cargo test
```

Expected: all 252 + all JIT unit tests passing.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(jit): wire JIT dispatch loop + 1024-instruction peripheral gate in main.rs"
```

---

## Task 9: Regression Tests + Boot Smoke Test

**Files:**
- No code changes — verification only.

- [ ] **Step 1: Run the full test suite**

```bash
cargo test
```

Expected output (exact counts may vary by how many JIT unit tests were added):
```
test result: ok. 257 passed; 0 failed
```

The 252 existing tests (88 unit + 164 riscv-tests) must all pass. They call `cpu.step()` directly and never touch `JitCache`, so they exercise the interpreter path unchanged.

- [ ] **Step 2: Run JIT unit tests individually to confirm all pass**

```bash
cargo test jit_
```

Expected: all of these pass:
- `jit_cache_new_is_empty`
- `jit_cache_invalidate_clears_all`
- `jit_addi`
- `jit_add`
- `jit_lui`
- `jit_mul`
- `jit_div`
- `jit_load_store`
- `jit_branch_taken`
- `jit_branch_not_taken`
- `jit_jal`
- `jit_slow_path`

- [ ] **Step 3: Release build**

```bash
cargo build --release
```

Expected: clean build.

- [ ] **Step 4: Boot smoke test (requires images)**

If `images/fw_jump.elf`, `images/Image`, and `images/rootfs.img` are present:

```bash
printf 'uname -a\nhalt\n' | timeout 120 cargo run --release -- \
  --dtb \
  --kernel  images/Image \
  --initrd  images/rootfs.img \
  images/fw_jump.elf 2>/dev/null
```

Expected: same boot output as Phase 5, ending with:
```
Linux (none) 6.12.85+deb13-riscv64 #1 SMP ...
```

If images are not available, skip this step and note it.

- [ ] **Step 5: Commit final state**

```bash
git add -p   # review any remaining changes
git commit -m "test(jit): verify all 252 tests pass + document boot smoke test"
```

---

## Self-Review Checklist

**Spec coverage:**
- [x] `Cargo.toml`: `dynasmrt = "2.0"` — Task 1
- [x] `src/jit.rs` — `JitCache`, block compiler, callout helpers — Tasks 1–6
- [x] `src/lib.rs` — `pub mod jit` — Task 1
- [x] `src/cpu/mod.rs` — `pub regs`, `pub jit_invalidate: bool` — Tasks 1, 7
- [x] `src/cpu/execute.rs` — `jit.invalidate()` on `satp` write and `Sfence` — Task 7
- [x] `src/main.rs` — JIT dispatch + 1024-instruction peripheral gate — Task 8
- [x] Tier 1 integer R-type, I-type, W-variants, LUI/AUIPC — Task 3
- [x] M-extension MUL/DIV/REM + W-variants — Task 4
- [x] Loads LB/LH/LW/LD/LBU/LHU/LWU, stores SB/SH/SW/SD — Task 5
- [x] Branches BEQ/BNE/BLT/BGE/BLTU/BGEU, jumps JAL/JALR — Task 6
- [x] All 5 JIT unit tests from spec — Tasks 3–6
- [x] All 252 existing tests pass — Task 9
- [x] Boot smoke test — Task 9

**Spec items deliberately deferred:**
- `MULHSU`: implemented as slow-path fallback (complex 128-bit signed×unsigned; a follow-up can emit native code)
- MMU-aware load/store callouts: callouts bypass MMU and go directly to `cpu.bus`; appropriate for the physical-address JIT of Phase 6a. Full virtual-address callouts are a Phase 6b concern.
