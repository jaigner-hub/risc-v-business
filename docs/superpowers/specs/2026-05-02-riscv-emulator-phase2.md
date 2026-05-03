# RISC-V Emulator Phase 2 Design

**Date:** 2026-05-02
**Goal:** Pass `rv64um-p-*`, `rv64ua-p-*`, and `rv64mi-p-*` riscv-tests.

---

## Scope

Three subsystems, implemented in order:

| Task group | Subsystem | Test gate |
|------------|-----------|-----------|
| 1 | CSR struct refactor + M extension | `rv64um-p-*` |
| 2 | A extension (atomics) | `rv64ua-p-*` |
| 3 | M-mode privilege + trap delivery | `rv64mi-p-*` |

---

## Task Group 1: CSR struct refactor + M extension

### CSR storage: named struct

Replace `HashMap<u16, u64>` on `Cpu` with a new `src/cpu/csr.rs` module containing a `Csr` struct.

**New file: `src/cpu/csr.rs`**

```rust
pub struct Csr {
    pub mstatus:  u64,
    pub misa:     u64,   // hardwired RV64IMA = 0x8000_0000_0000_1101
    pub mie:      u64,
    pub mtvec:    u64,
    pub mscratch: u64,
    pub mepc:     u64,
    pub mcause:   u64,
    pub mtval:    u64,
    pub mip:      u64,
    // S-mode (used in Phase 3)
    pub stvec:    u64,
    pub sscratch: u64,
    pub sepc:     u64,
    pub scause:   u64,
    pub stval:    u64,
    pub satp:     u64,
}
```

`Csr::read(addr: u16) -> u64` and `Csr::write(addr: u16, val: u64)` are a `match` on address:
- Read-only CSRs (`mhartid=0`, `mvendorid=0`, `marchid=0`, `mimpid=0`, `misa`) return hardwired values from `read` and are no-ops in `write`.
- All other known CSRs dispatch to the corresponding struct field.
- Unknown addresses: `read` returns 0, `write` is a no-op.

The Phase 1 blanket `bits[11:10]=11` guard in `csr_write` is removed; explicit per-address handling replaces it.

**`src/cpu/mod.rs` changes:**
- Remove `use std::collections::HashMap` and `csrs: HashMap<u16, u64>`.
- Add `pub csr: Csr`.
- Replace `csr_read`/`csr_write` methods on `Cpu` with delegation to `self.csr.read()`/`self.csr.write()`.

### M extension (13 new instruction variants)

Opcodes: `0x33` (64-bit) and `0x3B` (W-variants), both with `funct7 = 0x01`.

**New `Instruction` variants:**

```rust
// 64-bit (opcode 0x33, funct7=1) — all { rd, rs1, rs2 }
Mul, Mulh, Mulhsu, Mulhu,
Div, Divu, Rem, Remu,

// 32-bit W-variants (opcode 0x3B, funct7=1) — all { rd, rs1, rs2 }
Mulw, Divw, Divuw, Remw, Remuw,
```

**Execution semantics** (Unprivileged Spec §7):

- **MUL:** lower 64 bits of signed×signed product. `((rs1 as i64 as i128) * (rs2 as i64 as i128)) as u64`
- **MULH:** upper 64 bits, signed×signed. `(((rs1 as i64 as i128) * (rs2 as i64 as i128)) >> 64) as u64`
- **MULHSU:** upper 64 bits, signed×unsigned. `(((rs1 as i64 as i128) * (rs2 as u128 as i128)) >> 64) as u64`
- **MULHU:** upper 64 bits, unsigned×unsigned. `(((rs1 as u128) * (rs2 as u128)) >> 64) as u64`
- **DIV/REM:** signed. Division by zero: quotient = `-1i64 as u64`, remainder = dividend. Overflow (`-2^63 / -1`): quotient = `-2^63 as u64`, remainder = 0.
- **DIVU/REMU:** unsigned. Division by zero: quotient = `u64::MAX`, remainder = dividend.
- **W-variants:** operate on lower 32 bits (cast rs1/rs2 to i32/u32 as appropriate), sign-extend result from bit 31 via `sext(_, 31)`. Division-by-zero and overflow rules apply to 32-bit range.

---

## Task Group 2: A extension (22 new instruction variants)

Opcode: `0x2F`. Decode on `funct5 = inst[31:27]` and width `funct3` (bit 0: `0`=word, `1`=double).

**New field on `Cpu`:** `pub reservation: Option<u64>`

**New `Instruction` variants:**

```rust
// all take { rd: usize, rs1: usize } or { rd: usize, rs1: usize, rs2: usize }
LrD, LrW,                               // { rd, rs1 }
ScD, ScW,                               // { rd, rs1, rs2 }
AmoswapD, AmoswapW,
AmoaddD,  AmoaddW,
AmoxorD,  AmoxorW,
AmoandD,  AmoandW,
AmoorD,   AmoorW,
AmominD,  AmominW,
AmomaxD,  AmomaxW,
AmominuD, AmominuW,
AmomaxuD, AmomaxuW,                     // all { rd, rs1, rs2 }
```

**Execution semantics:**

- **LR.D/W:** load 8/4 bytes from `rs1`; set `cpu.reservation = Some(cpu.reg(rs1))`; write to `rd`. LR.W sign-extends the loaded value from bit 31.
- **SC.D/W:** if `cpu.reservation == Some(cpu.reg(rs1))`, store `rs2` to `rs1`, write `0` to `rd`; else write `1` to `rd`. Always clear `cpu.reservation = None`.
- **AMOs:** load old value from `rs1`; compute new value; store new value to `rs1`; write old value to `rd`.
  - W-variants: sign-extend loaded value before operation; sign-extend result before storing.
  - Signed min/max (`Amomin`/`Amomax`): compare as `i64` (or `i32` for W).
  - Unsigned min/max (`Amominu`/`Amomaxu`): compare as `u64` (or `u32` for W).
- **Ordering bits** (`aq`/`rl`, `inst[26:25]`): decoded but ignored — single-hart has trivial ordering.

---

## Task Group 3: M-mode privilege + trap delivery

### mstatus bookkeeping

`mstatus` is stored as a `u64` in `Csr`. Helper methods on `Csr` read/write the relevant bit fields:

| Field | Bits | Meaning |
|-------|------|---------|
| MIE   | [3]  | Machine interrupt enable |
| MPIE  | [7]  | Previous MIE (saved on trap) |
| MPP   | [12:11] | Previous privilege mode |

On **trap entry** (any of: ECALL, EBREAK, illegal instruction, misaligned fetch, access fault):
```
MPIE ← MIE
MIE  ← 0
MPP  ← current privilege (M=3 for now)
mepc ← pc of trapping instruction
mcause ← cause code (see below)
mtval ← fault-specific value
pc   ← mtvec (direct mode: mtvec & !0b11)
```

On **MRET:**
```
MIE  ← MPIE
MPIE ← 1
privilege ← MPP  (stays M in Phase 2)
pc   ← mepc
```

### Trap cause dispatch in `step()`

`step()` is extended to catch errors and deliver traps rather than propagating them. A new error type `TrapCause` (or a dedicated variant in the existing error type) carries `(cause: u64, tval: u64)`:

| Condition | mcause | mtval |
|-----------|--------|-------|
| Misaligned PC (`pc & 3 != 0`) | 0 | pc |
| Instruction access fault (bus error on fetch) | 1 | pc |
| Illegal instruction | 2 | raw instruction bits |
| Breakpoint (EBREAK) | 3 | pc |
| Env-call from M-mode (ECALL) | 11 | 0 |

`decode()` introduces a new `Err` variant `IllegalInstruction(u32)` (carrying the raw bits) so `step()` can distinguish it from other errors and set `mtval` correctly.

### Read-only CSRs for `rv64mi-p-csr`

| CSR | Address | Value |
|-----|---------|-------|
| mhartid | 0xf14 | 0 |
| mvendorid | 0xf11 | 0 |
| marchid | 0xf12 | 0 |
| mimpid | 0xf13 | 0 |
| misa | 0x301 | `0x8000_0000_0000_1101` (RV64IMA) |

`misa` encoding: MXL=2 (RV64) in bits[63:62]; extension bits A(bit 0), I(bit 8), M(bit 12) set.

---

## File map

| File | Change |
|------|--------|
| `src/cpu/csr.rs` | **New** — `Csr` struct + `read`/`write` dispatch |
| `src/cpu/mod.rs` | Replace `HashMap` → `Csr`; add `reservation: Option<u64>` |
| `src/cpu/decode.rs` | Add M (13) + A (22) variants; `IllegalInstruction(u32)` error |
| `src/cpu/execute.rs` | Add M, A handlers; mstatus helpers; trap delivery in `step()` |
| `src/lib.rs` | Add `pub mod` for `cpu::csr` if needed |
| `build.rs` | Extend filter to include `rv64um-p-*`, `rv64ua-p-*`, `rv64mi-p-*` |
| `scripts/fetch-riscv-tests.sh` | Copy three new suites after existing `rv64ui` copy |

---

## Done criterion

```
cargo test rv64ui_p_   # must still pass (no regression)
cargo test rv64um_p_   # new: all pass after Task Group 1
cargo test rv64ua_p_   # new: all pass after Task Group 2
cargo test rv64mi_p_   # new: all pass after Task Group 3
```
