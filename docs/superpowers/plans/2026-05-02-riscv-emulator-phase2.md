# RISC-V Emulator Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pass all `rv64um-p-*`, `rv64ua-p-*`, and `rv64mi-p-*` riscv-tests without regressing `rv64ui-p-*`.

**Architecture:** Three task groups, each gated by a test suite. Task Group 1 adds the M extension (MUL/DIV/REM) and migrates CSR storage from `HashMap` to a named `Csr` struct. Task Group 2 adds the A extension (LR/SC/AMO atomics). Task Group 3 adds full M-mode trap delivery so the privilege tests pass.

**Tech Stack:** Rust, anyhow, RISC-V Unprivileged Spec §7 (M ext), §8 (A ext), Privileged Spec §3 (M-mode). Test ELFs vendored in `tests/riscv-tests/`.

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/cpu/csr.rs` | **Create** | `Csr` struct with named fields; `read`/`write` dispatch; hardwired RO CSRs |
| `src/cpu/mod.rs` | **Modify** | Replace `HashMap` with `Csr`; add `reservation: Option<u64>`; delegate `csr_read`/`csr_write` |
| `src/cpu/decode.rs` | **Modify** | Add 13 M variants, 22 A variants; change final wildcard to `IllegalInstruction` |
| `src/cpu/execute.rs` | **Modify** | Add M handlers, A handlers, `deliver_trap`, full `Mret`, updated `step` trap catching |
| `scripts/fetch-riscv-tests.sh` | **Modify** | Copy rv64um/ua/mi suites alongside existing rv64ui copy |
| `build.rs` | **Modify** | Extend filter to accept rv64um-p-*, rv64ua-p-*, rv64mi-p-* |

---

## Task Group 1: CSR struct refactor + M extension → `rv64um-p-*` pass

### Task 1: Create `src/cpu/csr.rs`

**Files:**
- Create: `src/cpu/csr.rs`

- [ ] **Step 1: Write the failing test**

Add tests at the bottom of the new file (write the whole file at once):

```rust
// src/cpu/csr.rs
pub struct Csr {
    pub mstatus:  u64,
    pub misa:     u64,
    pub mie:      u64,
    pub mtvec:    u64,
    pub mscratch: u64,
    pub mepc:     u64,
    pub mcause:   u64,
    pub mtval:    u64,
    pub mip:      u64,
    pub stvec:    u64,
    pub sscratch: u64,
    pub sepc:     u64,
    pub scause:   u64,
    pub stval:    u64,
    pub satp:     u64,
}

impl Csr {
    pub fn new() -> Self {
        Self {
            mstatus:  0,
            misa:     0x8000_0000_0000_1101,
            mie:      0,
            mtvec:    0,
            mscratch: 0,
            mepc:     0,
            mcause:   0,
            mtval:    0,
            mip:      0,
            stvec:    0,
            sscratch: 0,
            sepc:     0,
            scause:   0,
            stval:    0,
            satp:     0,
        }
    }

    pub fn read(&self, addr: u16) -> u64 {
        match addr {
            0x300 => self.mstatus,
            0x301 => 0x8000_0000_0000_1101, // misa: hardwired RV64IMA
            0x304 => self.mie,
            0x305 => self.mtvec,
            0x340 => self.mscratch,
            0x341 => self.mepc,
            0x342 => self.mcause,
            0x343 => self.mtval,
            0x344 => self.mip,
            0x105 => self.stvec,
            0x140 => self.sscratch,
            0x141 => self.sepc,
            0x142 => self.scause,
            0x143 => self.stval,
            0x180 => self.satp,
            // Read-only: hardwired zero
            0xf11 => 0, // mvendorid
            0xf12 => 0, // marchid
            0xf13 => 0, // mimpid
            0xf14 => 0, // mhartid
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, val: u64) {
        match addr {
            0x300 => self.mstatus  = val,
            0x301 => {}            // misa: read-only
            0x304 => self.mie      = val,
            0x305 => self.mtvec    = val,
            0x340 => self.mscratch = val,
            0x341 => self.mepc     = val,
            0x342 => self.mcause   = val,
            0x343 => self.mtval    = val,
            0x344 => self.mip      = val,
            0x105 => self.stvec    = val,
            0x140 => self.sscratch = val,
            0x141 => self.sepc     = val,
            0x142 => self.scause   = val,
            0x143 => self.stval    = val,
            0x180 => self.satp     = val,
            // Read-only: silently ignore
            0xf11 | 0xf12 | 0xf13 | 0xf14 => {}
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misa_hardwired() {
        let mut csr = Csr::new();
        assert_eq!(csr.read(0x301), 0x8000_0000_0000_1101);
        csr.write(0x301, 0);
        assert_eq!(csr.read(0x301), 0x8000_0000_0000_1101); // write ignored
    }

    #[test]
    fn mhartid_hardwired_zero() {
        let mut csr = Csr::new();
        assert_eq!(csr.read(0xf14), 0);
        csr.write(0xf14, 0xdead);
        assert_eq!(csr.read(0xf14), 0); // write ignored
    }

    #[test]
    fn mtvec_round_trips() {
        let mut csr = Csr::new();
        csr.write(0x305, 0x8000_1000);
        assert_eq!(csr.read(0x305), 0x8000_1000);
    }

    #[test]
    fn unknown_csr_reads_zero() {
        let csr = Csr::new();
        assert_eq!(csr.read(0x999), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test csr
```

Expected: `error[E0433]: failed to resolve: use of undeclared crate or module 'csr'` — the module doesn't exist yet, which is correct.

- [ ] **Step 3: Write the file**

Write the content shown in Step 1 to `src/cpu/csr.rs`.

- [ ] **Step 4: Declare the module in `src/cpu/mod.rs`**

Add `pub mod csr;` as the first line of `src/cpu/mod.rs`:

```rust
pub mod csr;
pub mod decode;
pub mod execute;
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test csr
```

Expected: `test cpu::csr::tests::misa_hardwired ... ok` (4 tests pass)

- [ ] **Step 6: Commit**

```bash
git add src/cpu/csr.rs src/cpu/mod.rs
git commit -m "feat: add Csr struct with named fields and read/write dispatch"
```

---

### Task 2: Replace `HashMap` with `Csr` in `src/cpu/mod.rs`

**Files:**
- Modify: `src/cpu/mod.rs`

- [ ] **Step 1: Replace the HashMap import and field**

In `src/cpu/mod.rs`, remove:
```rust
use std::collections::HashMap;
```

Change the `Cpu` struct to replace:
```rust
    pub csrs: HashMap<u16, u64>,
```
with:
```rust
    pub csr: Csr,
    pub reservation: Option<u64>,
```

Also add `use crate::cpu::csr::Csr;` near the top (or use the path directly — the module is already `pub mod csr;` in the same file, so `use csr::Csr;` works).

The full updated `Cpu` struct and `new`:

```rust
use csr::Csr;

pub struct Cpu {
    regs:   [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
    pub csr: Csr,
    pub reservation: Option<u64>,
}

impl Cpu {
    pub fn new(bus: Bus, entry: u64, trace: bool) -> Self {
        Self {
            regs: [0u64; 32],
            pc: entry,
            bus,
            tracer: Tracer::new(trace),
            csr: Csr::new(),
            reservation: None,
        }
    }
```

- [ ] **Step 2: Update `csr_read` and `csr_write`**

Replace the two methods with delegation:

```rust
    #[inline]
    pub fn csr_read(&self, addr: u16) -> u64 {
        self.csr.read(addr)
    }

    #[inline]
    pub fn csr_write(&mut self, addr: u16, val: u64) {
        self.csr.write(addr, val);
    }
```

- [ ] **Step 3: Run all tests**

```bash
cargo test
```

Expected: all existing tests pass (including all rv64ui-p-* tests). No regressions.

- [ ] **Step 4: Commit**

```bash
git add src/cpu/mod.rs
git commit -m "refactor: replace HashMap CSR storage with named Csr struct"
```

---

### Task 3: Add M extension variants to `decode.rs`

**Files:**
- Modify: `src/cpu/decode.rs`

- [ ] **Step 1: Write failing decode tests**

Add to the `#[cfg(test)]` block in `src/cpu/decode.rs`:

```rust
    // MUL x3, x1, x2  →  0x0210_81B3
    #[test] fn decode_mul() {
        let inst = decode(0x021081B3).unwrap();
        assert_eq!(inst, Instruction::Mul { rd: 3, rs1: 1, rs2: 2 });
    }

    // MULW x3, x1, x2  →  0x0210_81BB
    #[test] fn decode_mulw() {
        let inst = decode(0x021081BB).unwrap();
        assert_eq!(inst, Instruction::Mulw { rd: 3, rs1: 1, rs2: 2 });
    }

    // DIV x3, x1, x2  →  0x0220_C1B3
    #[test] fn decode_div() {
        let inst = decode(0x0220C1B3).unwrap();
        assert_eq!(inst, Instruction::Div { rd: 3, rs1: 1, rs2: 2 });
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test decode_mul decode_mulw decode_div
```

Expected: compile error — `Instruction::Mul` does not exist.

- [ ] **Step 3: Add M variants to `Instruction` enum**

After the existing `And` variant and before the `Addw` variants:

```rust
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
```

- [ ] **Step 4: Add M decoding to the `match opcode` arms**

In the `0x33 =>` arm, add `funct7=0x01` cases before the wildcard:

```rust
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
```

In the `0x3B =>` arm, add W M-extension cases:

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test decode_mul decode_mulw decode_div
```

Expected: all 3 pass.

```bash
cargo test
```

Expected: all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/cpu/decode.rs
git commit -m "feat: add M extension instruction variants to decoder"
```

---

### Task 4: Add M extension execute handlers

**Files:**
- Modify: `src/cpu/execute.rs`

- [ ] **Step 1: Write failing execute tests**

Add to the `#[cfg(test)]` block in `src/cpu/execute.rs`:

```rust
    #[test] fn mul_lower64() {
        let mut c = cpu_with_ram(64);
        c.set_reg(1, (-1i64) as u64);  // 0xFFFF...FFFF
        c.set_reg(2, 2u64);
        execute(&mut c, Instruction::Mul { rd: 3, rs1: 1, rs2: 2 }).unwrap();
        assert_eq!(c.reg(3), (-2i64) as u64); // lower 64 of -2
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
        // lower 32 bits: 0x8000_0001 * 2 = 0x1_0000_0002; lower 32 = 0x0000_0002; sign-extend from bit31 = 2
        assert_eq!(c.reg(3), 2u64);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test mul_lower64 div_signed_by_zero
```

Expected: compile error — unmatched `Instruction::Mul` etc. in execute match.

- [ ] **Step 3: Add M extension execute handlers**

After the `And` arm in `execute()`, before the W-variant arms, add:

```rust
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
        // W-variants: operate on lower 32 bits, sign-extend result from bit 31
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
```

- [ ] **Step 4: Run tests**

```bash
cargo test mul div rem mulw divw
```

Expected: all M extension tests pass.

```bash
cargo test
```

Expected: all existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/cpu/execute.rs
git commit -m "feat: implement M extension (MUL/DIV/REM) execute handlers"
```

---

### Task 5: Vendor rv64um ELFs, extend build.rs, run the test gate

**Files:**
- Modify: `scripts/fetch-riscv-tests.sh`
- Modify: `build.rs`

- [ ] **Step 1: Extend the fetch script**

In `scripts/fetch-riscv-tests.sh`, after the existing `find ... rv64ui-p-*` line, add copies for all three new suites:

```bash
echo "Copying rv64ui-p-* to $DEST..."
mkdir -p "$DEST"
find "$WORK/riscv-tests/isa" -name 'rv64ui-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64um-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64um-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64ua-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64ua-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64mi-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64mi-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Done."
echo "  rv64ui: $(ls "$DEST" | grep -c 'rv64ui-p-') ELFs"
echo "  rv64um: $(ls "$DEST" | grep -c 'rv64um-p-') ELFs"
echo "  rv64ua: $(ls "$DEST" | grep -c 'rv64ua-p-') ELFs"
echo "  rv64mi: $(ls "$DEST" | grep -c 'rv64mi-p-') ELFs"
```

Also remove (or update) the old final `echo "Done. ... rv64ui-p-..." line`.

- [ ] **Step 2: Run the fetch script (Docker)**

Since the toolchain isn't installed natively, use Docker (as established in Phase 1):

```bash
docker run --rm -v "$(pwd):/work" -w /work riscv-toolchain bash scripts/fetch-riscv-tests.sh
```

Expected output ends with counts for all four suites.

- [ ] **Step 3: Extend `build.rs` filter to accept rv64um-p-***

Change the filter line in `build.rs` from:

```rust
            if !name.starts_with("rv64ui-p-") || name.ends_with(".dump") || name.ends_with(".o") || name == ".gitkeep" {
```

to:

```rust
            let is_test_elf = name.starts_with("rv64ui-p-")
                || name.starts_with("rv64um-p-")
                || name.starts_with("rv64ua-p-")
                || name.starts_with("rv64mi-p-");
            if !is_test_elf || name.ends_with(".dump") || name.ends_with(".o") || name == ".gitkeep" {
```

- [ ] **Step 4: Run the rv64um test gate**

```bash
cargo test rv64um_p_
```

Expected: all rv64um-p-* tests pass (typically 8 tests: mul, mulh, mulhsu, mulhu, div, divu, rem, remu).

Also verify no regressions:

```bash
cargo test rv64ui_p_
```

Expected: all 54 rv64ui-p-* tests still pass.

- [ ] **Step 5: Commit**

```bash
git add tests/riscv-tests/ scripts/fetch-riscv-tests.sh build.rs
git commit -m "feat: vendor rv64um/ua/mi test ELFs; extend build.rs filter"
```

---

## Task Group 2: A extension → `rv64ua-p-*` pass

### Task 6: Add A extension variants to `decode.rs`

**Files:**
- Modify: `src/cpu/decode.rs`

- [ ] **Step 1: Write failing decode tests**

Add to the `#[cfg(test)]` block:

```rust
    // LR.D x3, (x1)  → opcode=0x2F, funct3=3, funct5=0x02, rs2=0, rd=3, rs1=1
    // 0001 0000 0000 0000 1011 0001 1010 1111 = 0x1000_B1AF
    #[test] fn decode_lrd() {
        let inst = decode(0x1000B1AF).unwrap();
        assert_eq!(inst, Instruction::LrD { rd: 3, rs1: 1 });
    }

    // SC.D x3, x1, x2 → funct5=0x03, funct3=3, rd=3, rs1=1, rs2=2
    // 0001 1000 0010 0000 1011 0001 1010 1111 = 0x1820_B1AF
    #[test] fn decode_scd() {
        let inst = decode(0x1820B1AF).unwrap();
        assert_eq!(inst, Instruction::ScD { rd: 3, rs1: 1, rs2: 2 });
    }

    // AMOADD.D x3, x1, x2 → funct5=0x00, funct3=3, rd=3, rs1=1, rs2=2
    // 0000 0000 0010 0000 1011 0001 1010 1111 = 0x0020_B1AF
    #[test] fn decode_amoaddd() {
        let inst = decode(0x0020B1AF).unwrap();
        assert_eq!(inst, Instruction::AmoaddD { rd: 3, rs1: 1, rs2: 2 });
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test decode_lrd decode_scd decode_amoaddd
```

Expected: compile error — `Instruction::LrD` does not exist.

- [ ] **Step 3: Add A variants to `Instruction` enum**

Add after the M extension variants (before the I-type variants):

```rust
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
```

- [ ] **Step 4: Add opcode 0x2F decode arm**

Add before the final `_ =>` wildcard in the outer `match opcode`:

```rust
        // A extension: opcode 0x2F
        // funct5 = inst[31:27]; funct3 bit0: 0=word, 1=double; aq/rl ignored.
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
                _ => Err(anyhow!("illegal AMO funct5={funct5:#x} funct3={funct3:#x}")),
            }
        },
```

- [ ] **Step 5: Run tests**

```bash
cargo test decode_lrd decode_scd decode_amoaddd
```

Expected: all 3 pass.

```bash
cargo test
```

Expected: all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/cpu/decode.rs
git commit -m "feat: add A extension instruction variants to decoder"
```

---

### Task 7: Add A extension execute handlers

**Files:**
- Modify: `src/cpu/execute.rs`

- [ ] **Step 1: Write failing execute tests**

Add to the `#[cfg(test)]` block:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test lr_sc_success amoadd_d
```

Expected: compile error — unmatched `Instruction::LrD` etc.

- [ ] **Step 3: Add A extension execute handlers**

Add a helper macro and the AMO handlers after the M extension handlers:

```rust
        // --- A extension: atomics ---
        // Spec: Unprivileged §8. Single-hart: aq/rl ordering is trivially satisfied.
        Instruction::LrD { rd, rs1 } => {
            let addr = cpu.reg(rs1);
            let v = cpu.bus.load(addr, 8)?;
            cpu.reservation = Some(addr);
            cpu.set_reg(rd, v);
        },
        Instruction::LrW { rd, rs1 } => {
            let addr = cpu.reg(rs1);
            let v = sext(cpu.bus.load(addr, 4)?, 31);
            cpu.reservation = Some(addr);
            cpu.set_reg(rd, v);
        },
        Instruction::ScD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            if cpu.reservation == Some(addr) {
                cpu.bus.store(addr, 8, cpu.reg(rs2))?;
                cpu.set_reg(rd, 0);
            } else {
                cpu.set_reg(rd, 1);
            }
            cpu.reservation = None;
        },
        Instruction::ScW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            if cpu.reservation == Some(addr) {
                cpu.bus.store(addr, 4, cpu.reg(rs2))?;
                cpu.set_reg(rd, 0);
            } else {
                cpu.set_reg(rd, 1);
            }
            cpu.reservation = None;
        },
        Instruction::AmoswapD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            cpu.bus.store(addr, 8, cpu.reg(rs2))?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoswapW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            cpu.bus.store(addr, 4, cpu.reg(rs2))?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoaddD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            cpu.bus.store(addr, 8, old.wrapping_add(cpu.reg(rs2)))?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoaddW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let new = sext(old.wrapping_add(cpu.reg(rs2)), 31);
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoxorD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            cpu.bus.store(addr, 8, old ^ cpu.reg(rs2))?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoxorW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let new = sext(old ^ cpu.reg(rs2), 31);
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoandD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            cpu.bus.store(addr, 8, old & cpu.reg(rs2))?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoandW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let new = sext(old & cpu.reg(rs2), 31);
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoorD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            cpu.bus.store(addr, 8, old | cpu.reg(rs2))?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmoorW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let new = sext(old | cpu.reg(rs2), 31);
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmominD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            let new = if (old as i64) < (cpu.reg(rs2) as i64) { old } else { cpu.reg(rs2) };
            cpu.bus.store(addr, 8, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmominW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let rs2v = cpu.reg(rs2);
            let new = if (old as i32) < (rs2v as i32) { old } else { sext(rs2v, 31) };
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmomaxD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            let new = if (old as i64) > (cpu.reg(rs2) as i64) { old } else { cpu.reg(rs2) };
            cpu.bus.store(addr, 8, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmomaxW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let rs2v = cpu.reg(rs2);
            let new = if (old as i32) > (rs2v as i32) { old } else { sext(rs2v, 31) };
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmominuD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            let new = old.min(cpu.reg(rs2));
            cpu.bus.store(addr, 8, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmominuW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let rs2v = cpu.reg(rs2) as u32;
            let new = if (old as u32) < rs2v { old } else { sext(rs2v as u64, 31) };
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmomaxuD { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = cpu.bus.load(addr, 8)?;
            let new = old.max(cpu.reg(rs2));
            cpu.bus.store(addr, 8, new)?;
            cpu.set_reg(rd, old);
        },
        Instruction::AmomaxuW { rd, rs1, rs2 } => {
            let addr = cpu.reg(rs1);
            let old = sext(cpu.bus.load(addr, 4)?, 31);
            let rs2v = cpu.reg(rs2) as u32;
            let new = if (old as u32) > rs2v { old } else { sext(rs2v as u64, 31) };
            cpu.bus.store(addr, 4, new)?;
            cpu.set_reg(rd, old);
        },
```

- [ ] **Step 4: Run tests**

```bash
cargo test lr_sc sc_failure amoadd_d amomin_d
```

Expected: all 4 pass.

```bash
cargo test
```

Expected: all existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/cpu/execute.rs
git commit -m "feat: implement A extension (LR/SC/AMO) execute handlers"
```

---

### Task 8: Run the rv64ua test gate

**Files:**
- No new file changes needed (ELFs already vendored in Task 5, build.rs already updated)

- [ ] **Step 1: Run the rv64ua test gate**

```bash
cargo test rv64ua_p_
```

Expected: all rv64ua-p-* tests pass (typically 8 tests: amoadd, amoand, amoor, amoswap, amoxor, amomax, amomin, lr_sc variants for D and W).

Also verify no regressions:

```bash
cargo test rv64ui_p_ rv64um_p_
```

Expected: all pass.

- [ ] **Step 2: Commit if tests pass (no code changes, but a verification commit is fine to skip)**

No commit needed if there are no code changes.

---

## Task Group 3: M-mode privilege + trap delivery → `rv64mi-p-*` pass

### Task 9: Add `mstatus` helpers and fix `Mret` in `execute.rs`

**Files:**
- Modify: `src/cpu/csr.rs`
- Modify: `src/cpu/execute.rs`

- [ ] **Step 1: Write failing mstatus tests**

Add to `src/cpu/csr.rs` tests:

```rust
    #[test]
    fn mstatus_mie_mpie_mpp() {
        let mut csr = Csr::new();
        // Set MIE=1 (bit 3)
        csr.mstatus = 0b1000; // MIE=1
        assert_eq!(csr.mie_bit(), 1);
        // Simulate trap entry: MPIE←MIE, MIE←0, MPP←3
        csr.trap_entry();
        assert_eq!(csr.mie_bit(), 0);           // MIE cleared
        assert_eq!((csr.mstatus >> 7) & 1, 1);  // MPIE = previous MIE
        assert_eq!((csr.mstatus >> 11) & 3, 3); // MPP = M-mode
        // Simulate MRET: MIE←MPIE, MPIE←1
        csr.mret();
        assert_eq!(csr.mie_bit(), 1);           // MIE restored
        assert_eq!((csr.mstatus >> 7) & 1, 1);  // MPIE set to 1
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test mstatus_mie_mpie_mpp
```

Expected: compile error — `mie_bit`, `trap_entry`, `mret` don't exist on `Csr`.

- [ ] **Step 3: Add mstatus helpers to `Csr`**

Add to the `impl Csr` block in `src/cpu/csr.rs`:

```rust
    pub fn mie_bit(&self) -> u64 {
        (self.mstatus >> 3) & 1
    }

    /// Transition mstatus on trap entry: MPIE←MIE, MIE←0, MPP←3 (M-mode).
    pub fn trap_entry(&mut self) {
        let mie = self.mie_bit();
        self.mstatus = (self.mstatus & !0x1888u64) // clear MIE[3], MPIE[7], MPP[12:11]
            | (mie << 7)                           // MPIE ← MIE
            | (3u64 << 11);                        // MPP ← M-mode
    }

    /// Transition mstatus on MRET: MIE←MPIE, MPIE←1, MPP←U (0).
    pub fn mret(&mut self) {
        let mpie = (self.mstatus >> 7) & 1;
        self.mstatus = (self.mstatus & !0x1888u64) // clear MIE[3], MPIE[7], MPP[12:11]
            | (mpie << 3)                          // MIE ← MPIE
            | (1u64 << 7);                         // MPIE ← 1
    }
```

- [ ] **Step 4: Update `Mret` and `Ecall`/`Ebreak` in `execute.rs` to use mstatus helpers**

Change the `Instruction::Mret` arm from:

```rust
        Instruction::Mret => {
            next_pc = cpu.csr_read(CSR_MEPC);
        },
```

to:

```rust
        Instruction::Mret => {
            cpu.csr.mret();
            next_pc = cpu.csr.mepc;
        },
```

Also add `cpu.csr.trap_entry()` to the `Ecall` and `Ebreak` handlers (they already set mepc/mcause/mtval/next_pc, but they're missing the mstatus transition):

```rust
        Instruction::Ecall => {
            cpu.csr.trap_entry();
            cpu.csr_write(CSR_MCAUSE, 11);
            cpu.csr_write(CSR_MEPC, pc);
            cpu.csr_write(CSR_MTVAL, 0);
            next_pc = cpu.csr_read(CSR_MTVEC) & !0b11;
        },
        Instruction::Ebreak => {
            cpu.csr.trap_entry();
            cpu.csr_write(CSR_MCAUSE, 3);
            cpu.csr_write(CSR_MEPC, pc);
            cpu.csr_write(CSR_MTVAL, pc);
            next_pc = cpu.csr_read(CSR_MTVEC) & !0b11;
        },
```

- [ ] **Step 5: Add `deliver_trap` helper to `Cpu` in `src/cpu/mod.rs`**

Add this method to `impl Cpu`:

```rust
    /// Deliver a synchronous trap: update mstatus, mepc, mcause, mtval, jump to mtvec.
    pub fn deliver_trap(&mut self, cause: u64, tval: u64) {
        self.csr.trap_entry();
        self.csr.mepc   = self.pc;
        self.csr.mcause = cause;
        self.csr.mtval  = tval;
        self.pc = self.csr.mtvec & !0b11;
    }
```

- [ ] **Step 6: Run tests**

```bash
cargo test mstatus_mie_mpie_mpp
cargo test
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/cpu/csr.rs src/cpu/execute.rs src/cpu/mod.rs
git commit -m "feat: add mstatus trap_entry/mret helpers; fix Mret mstatus bookkeeping"
```

---

### Task 10: Change `decode()` illegal instruction to `IllegalInstruction(u32)`

**Files:**
- Modify: `src/cpu/decode.rs`

- [ ] **Step 1: Write failing test**

Add to `src/cpu/decode.rs` tests:

```rust
    #[test]
    fn illegal_instruction_carries_raw_bits() {
        // 0xDEAD_BEEF is not a valid instruction
        let err = decode(0xDEADBEEF).unwrap_err();
        let ill = err.downcast_ref::<IllegalInstruction>()
            .expect("expected IllegalInstruction error");
        assert_eq!(ill.0, 0xDEADBEEF);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test illegal_instruction_carries_raw_bits
```

Expected: compile error — `IllegalInstruction` doesn't exist.

- [ ] **Step 3: Add the `IllegalInstruction` error type and update the wildcard**

Add near the top of `src/cpu/decode.rs` (after the `use` statements):

```rust
/// Returned by `decode()` for unrecognized instruction encodings.
/// The `u32` payload is the raw instruction word, used by `step()` to set `mtval`.
#[derive(Debug)]
pub struct IllegalInstruction(pub u32);

impl std::fmt::Display for IllegalInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal instruction: {:#010x}", self.0)
    }
}

impl std::error::Error for IllegalInstruction {}
```

Change every `Err(anyhow!(...))` in decode arms that indicate an illegal/unknown encoding to use `IllegalInstruction`. Specifically:

- The final `_ =>` wildcard at the bottom of `match opcode`:
  ```rust
  _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
  ```
- The `_ =>` wildcards inside `0x33`, `0x3B`, `0x2F`, `0x63`, `0x73` (funct3=0 bad system), and other inner match arms that signal an illegal encoding should also become `IllegalInstruction`:
  ```rust
  // In 0x33 arm wildcard:
  _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
  // In 0x3B arm wildcard:
  _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
  // In 0x2F arm wildcard:
  _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
  // In 0x73 funct3=0 arm wildcard:
  _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
  // In 0x73 funct3 wildcard:
  _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
  // Outer wildcard:
  _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
  ```

Leave `Err(anyhow!(...))` for the non-opcode wildcards that can't actually be reached (like the `0x13` funct3 `_ => unreachable!()`).

- [ ] **Step 4: Run tests**

```bash
cargo test illegal_instruction_carries_raw_bits
cargo test
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/cpu/decode.rs
git commit -m "feat: introduce IllegalInstruction(u32) error type from decode()"
```

---

### Task 11: Full trap delivery in `step()`

**Files:**
- Modify: `src/cpu/mod.rs`

- [ ] **Step 1: Write failing test**

Add to `src/cpu/mod.rs` tests:

```rust
    #[test]
    fn step_delivers_illegal_instruction_trap() {
        use crate::bus::Bus;
        let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
        // Write mtvec = 0x8000_0010 (direct mode)
        c.csr.mtvec = 0x8000_0010;
        // Write an illegal instruction (0xDEAD_BEEF) at PC
        c.bus.store(0x8000_0000, 4, 0xDEAD_BEEF).unwrap();
        // step() should NOT return an error; it should deliver the trap
        c.step().unwrap();
        assert_eq!(c.pc, 0x8000_0010);          // jumped to mtvec
        assert_eq!(c.csr.mcause, 2);            // illegal instruction
        assert_eq!(c.csr.mtval, 0xDEAD_BEEF);  // raw bits
        assert_eq!(c.csr.mepc, 0x8000_0000);   // PC of faulting instruction
    }

    #[test]
    fn step_delivers_fetch_fault_on_bad_address() {
        use crate::bus::Bus;
        let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
        c.csr.mtvec = 0x8000_0010;
        // Point PC at an unmapped address (outside RAM)
        c.pc = 0x0000_0000;
        c.step().unwrap();
        assert_eq!(c.pc, 0x8000_0010);
        assert_eq!(c.csr.mcause, 1); // instruction access fault
        assert_eq!(c.csr.mtval, 0x0000_0000);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test step_delivers_illegal_instruction_trap step_delivers_fetch_fault
```

Expected: tests fail — `step()` currently returns `Err` instead of delivering the trap.

- [ ] **Step 3: Update `step()` to catch trappable errors**

Replace the body of `step()` in `src/cpu/mod.rs` with:

```rust
    pub fn step(&mut self) -> Result<()> {
        use decode::{decode, IllegalInstruction};
        use execute::execute;

        // --- Instruction fetch ---
        // Bus error → mcause=1 (instruction access fault).
        let pc = self.pc;
        let raw = match self.bus.load(pc, 4) {
            Ok(v) => v as u32,
            Err(_) => {
                self.deliver_trap(1, pc);
                return Ok(());
            }
        };

        // --- Decode ---
        // IllegalInstruction → mcause=2, mtval=raw bits.
        let inst = match decode(raw) {
            Ok(i) => i,
            Err(e) => {
                let tval = if e.downcast_ref::<IllegalInstruction>().is_some() {
                    raw as u64
                } else {
                    0
                };
                self.deliver_trap(2, tval);
                return Ok(());
            }
        };

        // --- Execute ---
        if self.tracer.enabled {
            let before = self.regs;
            let pc = self.pc;
            let mnemonic = format!("{inst:?}");
            let short = mnemonic.split(' ').next().unwrap_or(&mnemonic).to_owned();
            execute(self, inst)?;
            let changes: Vec<(usize, u64, u64)> = (1..32)
                .filter(|&i| before[i] != self.regs[i])
                .map(|i| (i, before[i], self.regs[i]))
                .collect();
            self.tracer.trace_step(pc, raw, &short, "", &changes);
        } else {
            execute(self, inst)?;
        }

        Ok(())
    }
```

Note: execute errors (bus load/store faults) still propagate as `Err` for now — the rv64mi tests don't require trapping those. They can be added in Phase 3 if needed.

- [ ] **Step 4: Run tests**

```bash
cargo test step_delivers_illegal_instruction_trap step_delivers_fetch_fault
cargo test
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/cpu/mod.rs
git commit -m "feat: step() catches illegal instruction and fetch faults, delivers traps"
```

---

### Task 12: Run the rv64mi test gate

**Files:**
- No new file changes (ELFs already vendored in Task 5, build.rs already updated)

- [ ] **Step 1: Run the rv64mi test gate**

```bash
cargo test rv64mi_p_
```

Expected: all rv64mi-p-* tests pass (typically: access, breakpoint, csr, illegal, ma_addr, ma_fetch, mcsr, sbreak, shamt).

- [ ] **Step 2: Run the full Phase 2 done criterion**

```bash
cargo test rv64ui_p_
cargo test rv64um_p_
cargo test rv64ua_p_
cargo test rv64mi_p_
```

Expected: all four suites pass with zero failures.

- [ ] **Step 3: Final commit**

```bash
git commit --allow-empty -m "test: Phase 2 complete — rv64um/ua/mi all pass"
```

---

## Done Criterion

```
cargo test rv64ui_p_   # no regression (54 tests)
cargo test rv64um_p_   # M extension (8 tests)
cargo test rv64ua_p_   # A extension (~8 tests)
cargo test rv64mi_p_   # M-mode privilege (~9 tests)
```
