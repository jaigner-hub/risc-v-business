# Phase 6a — x86-64 JIT + Peripheral Batching

## Goal

Replace the per-instruction interpret loop with a basic-block JIT that emits real x86-64 machine code for integer/arithmetic instructions and batches peripheral polling from every instruction to every 1024 blocks. Target: meaningfully faster boot and interactive feel without breaking any existing tests.

## Architecture

Two independent changes shipped together:

1. **`src/jit.rs`** — basic block cache and x86-64 code emitter using `dynasmrt`.
2. **`src/main.rs`** — updated run loop: JIT dispatch + 1024-instruction peripheral gate.

Nothing in `src/cpu/` changes. The interpreter remains intact and is the fallback for all unhandled instructions and slow-path exits.

## File Layout

| File | Change |
|------|--------|
| `Cargo.toml` | add `dynasm = "2.0"`, `dynasmrt = "2.0"` |
| `src/jit.rs` | new — `JitCache`, block compiler, mem callout helpers |
| `src/lib.rs` | add `pub mod jit` |
| `src/main.rs` | updated run loop |
| `src/cpu/execute.rs` | add `jit.invalidate()` call on `satp` write and `Sfence` |

## JIT Calling Convention

Each compiled block has this Rust type:

```rust
type JitFn = unsafe extern "sysv64" fn(regs: *mut u64, cpu: *mut Cpu) -> u64;
```

- `rdi` (first arg) = `&mut cpu.regs[0]` — the 32-element `u64` register file.
- `rsi` (second arg) = `*mut Cpu` — passed through to memory callout helpers.
- Return value (`rax`) = next guest PC, or `u64::MAX` = slow path (trap, CSR, unhandled instruction). The run loop interprets one step on slow-path return.

### Dedicated registers inside a block

| x86-64 | Holds | Notes |
|--------|-------|-------|
| `r15` | RISC-V register file base | callee-saved; set from `rdi` on entry |
| `r14` | `*mut Cpu` | callee-saved; set from `rsi` on entry |

Block prologue: `push r15 / push r14 / mov r15, rdi / mov r14, rsi`.
Block epilogue: `pop r14 / pop r15 / mov rax, <next_pc> / ret`.

### Register file access

```asm
mov rax, [r15 + rs1*8]    ; load xRS1 into rax
mov [r15 + rd*8],  rax    ; write rax to xRD  (omitted when rd == 0)
```

## Block Compilation

### Basic block boundaries

Compilation starts at a guest PC and walks forward, decoding with the existing `decode()` / `decode_rvc()`. A block ends at the first of:

- Any branch or jump (BEQ/BNE/BLT/BGE/BLTU/BGEU, JAL, JALR)
- ECALL, EBREAK, any CSR instruction, WFI, FENCE
- Any instruction not in Tier 1 (emit slow-path return immediately)
- 64 instructions reached — emit fall-through exit with `pc + 64*4`

All block exits emit: epilogue + `mov rax, <next_pc>` + `ret`. Slow-path exits use `mov rax, -1` (i.e., `u64::MAX`).

### Tier 1 — JITted natively

| Group | Instructions |
|-------|-------------|
| RV64I integer R-type | ADD SUB AND OR XOR SLL SRL SRA SLT SLTU |
| RV64I integer I-type | ADDI ANDI ORI XORI SLLI SRLI SRAI SLTI SLTIU |
| RV64I W-variants | ADDW SUBW SLLW SRLW SRAW ADDIW SLLIW SRLIW SRAIW |
| Upper immediates | LUI AUIPC |
| M-extension | MUL MULH MULHSU MULHU MULW / DIV DIVU DIVW DIVUW / REM REMU REMW REMUW |
| Loads | LB LH LW LD LBU LHU LWU — via `jit_load_N` callout |
| Stores | SB SH SW SD — via `jit_store_N` callout |
| Jumps | JAL JALR — end block, return target PC |
| Branches | BEQ BNE BLT BGE BLTU BGEU — end block, return taken or fall-through PC |

### Slow path (interpreter fallback)

CSR instructions, ECALL, EBREAK, FENCE, WFI, atomics (A-extension), FP (F/D-extension), RVC. These return `u64::MAX`; the run loop calls `cpu.step()` for that one instruction.

### Memory callout helpers

```rust
// In src/jit.rs, exported as extern "sysv64"
unsafe extern "sysv64" fn jit_load8 (cpu: *mut Cpu, va: u64) -> u64
unsafe extern "sysv64" fn jit_load16(cpu: *mut Cpu, va: u64) -> u64
unsafe extern "sysv64" fn jit_load32(cpu: *mut Cpu, va: u64) -> u64
unsafe extern "sysv64" fn jit_load64(cpu: *mut Cpu, va: u64) -> u64
unsafe extern "sysv64" fn jit_store8 (cpu: *mut Cpu, va: u64, val: u64)
unsafe extern "sysv64" fn jit_store16(cpu: *mut Cpu, va: u64, val: u64)
unsafe extern "sysv64" fn jit_store32(cpu: *mut Cpu, va: u64, val: u64)
unsafe extern "sysv64" fn jit_store64(cpu: *mut Cpu, va: u64, val: u64)
```

Each calls `cpu.mmu.translate()` then `cpu.bus.load/store()`, matching exactly what the interpreter does. Load helpers return the loaded value in `rax`; store helpers return `0` on success or `u64::MAX` on fault. After every load/store `call` in the JIT, the emitter checks `rax == -1`; if so it emits `ret` with `rax = u64::MAX` (slow path). The run loop then calls `cpu.step()` to re-execute the faulting instruction under the interpreter, which delivers the trap correctly.

## JitCache

```rust
pub struct JitCache {
    blocks: HashMap<u64, (ExecutableBuffer, JitFn)>,
}

impl JitCache {
    pub fn new() -> Self
    pub fn get(&self, pc: u64) -> Option<JitFn>
    pub fn compile(&mut self, cpu: &mut Cpu, pc: u64)
    pub fn invalidate(&mut self)   // called on satp write and sfence.vma
}
```

`compile()` decodes the block starting at `pc` using the existing decoder, emits x86-64 via `dynasmrt`, finalises the buffer (makes it executable), and inserts into `blocks`.

## Updated Run Loop (`src/main.rs`)

```
let mut jit = JitCache::new();
let mut tick: u64 = 0;

loop {
    // JIT dispatch
    match jit.get(cpu.pc) {
        Some(f) => {
            let next = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
            if next == u64::MAX {
                cpu.step()?;          // slow path
            } else {
                cpu.pc = next;
            }
        }
        None => {
            cpu.step()?;              // interpret + schedule compile
            jit.compile(&mut cpu, cpu.pc);
        }
    }

    // Peripheral checks gated to every 1024 iterations
    tick += 1;
    if tick & 1023 == 0 {
        // CLINT / STIP / SEIP / stdin drain (moved verbatim from old loop)
    }
}
```

## Cache Invalidation

| Trigger | Action |
|---------|--------|
| CSR write to `satp` | `jit.invalidate()` — full cache flush |
| `SFENCE.VMA` | `jit.invalidate()` — full cache flush |

Both are detected in the existing `execute()` CSR/Sfence arms. A `pub jit_invalidate: bool` flag is added to `Cpu`; the `satp` write arm and `Sfence` arm set it to `true`. The run loop checks and clears the flag after every step, calling `jit.invalidate()` when set. This avoids changing the `execute()` signature.

## Testing

1. **All 252 existing tests pass** — no changes to the test harness; riscv-tests use `cpu.step()` directly and never touch `JitCache`.
2. **JIT unit tests in `src/jit.rs`**:
   - `jit_add`: compile `addi x1, x0, 42` → assert `regs[1] == 42`
   - `jit_branch_taken`: compile `bne x1, x0, +8` with `x1 != 0` → assert returned PC = entry+8
   - `jit_branch_not_taken`: same with `x1 == 0` → assert returned PC = entry+4
   - `jit_load_store`: compile `sw x1, 0(x2)` + `lw x3, 0(x2)` → assert `x3 == x1`
   - `jit_slow_path`: compile `csrr x1, mhartid` → assert returns `u64::MAX`
3. **Boot smoke test**: `printf 'uname -a\n' | timeout 120 cargo run --release -- --dtb ...` produces the same output as Phase 5.
