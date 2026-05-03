# RISC-V Emulator Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a RV64I user-mode interpreter that passes all `rv64ui-p-*` riscv-tests.

**Architecture:** A `Cpu` struct owns a `Bus` (RAM-only in Phase 1) and a `Tracer`. `decode(u32) -> Instruction` turns raw bits into a typed enum; `execute(&mut Cpu, Instruction)` applies it. The test harness loads vendored ELFs, watches for a `tohost` store, and asserts the value is 1.

**Tech Stack:** Rust 2021 edition, clap 4 (derive), anyhow 1, goblin 0.8.

---

## File map

| File | Responsibility |
|------|----------------|
| `Cargo.toml` | Package + deps |
| `build.rs` | Generates one `#[test]` per vendored ELF |
| `src/lib.rs` | `pub mod` re-exports (required so `tests/` can import) |
| `src/main.rs` | CLI entry: parse args, load ELF, run loop |
| `src/bus.rs` | `Bus` struct — little-endian RAM load/store |
| `src/loader.rs` | ELF64 loader: PT_LOAD segments + symbol table lookup |
| `src/cpu/mod.rs` | `Cpu` struct, `step()`, reg accessors, `Tracer` |
| `src/cpu/decode.rs` | `Instruction` enum + `decode(u32) -> Result<Instruction>` |
| `src/cpu/execute.rs` | `execute(&mut Cpu, Instruction) -> Result<()>` |
| `tests/riscv_tests.rs` | Integration test harness |
| `tests/riscv-tests/` | Vendored `rv64ui-p-*` ELF binaries |
| `scripts/fetch-riscv-tests.sh` | One-time script to build+vendor the test ELFs |
| `.gitignore` | `images/`, `target/` |

---

## Task 1: Cargo scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `build.rs`
- Create: `src/lib.rs`
- Create: `src/main.rs`
- Create: `src/bus.rs`
- Create: `src/loader.rs`
- Create: `src/cpu/mod.rs`
- Create: `src/cpu/decode.rs`
- Create: `src/cpu/execute.rs`
- Create: `.gitignore`

- [ ] **Create `Cargo.toml`**

```toml
[package]
name = "riscv-emu"
version = "0.1.0"
edition = "2021"

[lib]
name = "riscv_emu"
path = "src/lib.rs"

[[bin]]
name = "riscv-emu"
path = "src/main.rs"

[dependencies]
clap    = { version = "4", features = ["derive"] }
anyhow  = "1"
goblin  = "0.8"

[profile.release]
opt-level = 3
```

- [ ] **Create `build.rs`** (stub — will be filled in Task 9)

```rust
fn main() {}
```

- [ ] **Create `src/lib.rs`**

```rust
pub mod bus;
pub mod cpu;
pub mod loader;
```

- [ ] **Create `src/main.rs`** (stub)

```rust
fn main() {}
```

- [ ] **Create `src/bus.rs`** (stub)

```rust
pub struct Bus;
```

- [ ] **Create `src/loader.rs`** (stub)

```rust
pub struct LoadedElf;
```

- [ ] **Create `src/cpu/mod.rs`** (stub)

```rust
pub mod decode;
pub mod execute;
```

- [ ] **Create `src/cpu/decode.rs`** (stub)

```rust
pub enum Instruction {}
```

- [ ] **Create `src/cpu/execute.rs`** (stub)

```rust
```

- [ ] **Create `.gitignore`**

```
/target
/images
```

- [ ] **Verify project compiles**

```bash
cargo build
```

Expected: compiles with warnings about unused stubs, no errors.

- [ ] **Commit**

```bash
git add -A
git commit -m "chore: scaffold Phase 1 project structure"
```

---

## Task 2: Bus — little-endian RAM load/store

**Files:**
- Modify: `src/bus.rs`

- [ ] **Replace `src/bus.rs` with full implementation**

```rust
use anyhow::{anyhow, Result};

pub struct Bus {
    ram: Vec<u8>,
    pub ram_base: u64,
}

impl Bus {
    pub fn new(size: usize, ram_base: u64) -> Self {
        Self { ram: vec![0u8; size], ram_base }
    }

    pub fn ram_mut(&mut self) -> &mut Vec<u8> {
        &mut self.ram
    }

    /// Load `width` bytes (1/2/4/8) from `addr`, zero-extended to u64.
    pub fn load(&self, addr: u64, width: usize) -> Result<u64> {
        let off = self.offset(addr, width)?;
        Ok(match width {
            1 => self.ram[off] as u64,
            2 => u16::from_le_bytes(self.ram[off..off+2].try_into().unwrap()) as u64,
            4 => u32::from_le_bytes(self.ram[off..off+4].try_into().unwrap()) as u64,
            8 => u64::from_le_bytes(self.ram[off..off+8].try_into().unwrap()),
            _ => unreachable!("invalid load width {width}"),
        })
    }

    /// Store `width` bytes (1/2/4/8) of `value` to `addr`.
    pub fn store(&mut self, addr: u64, width: usize, value: u64) -> Result<()> {
        let off = self.offset(addr, width)?;
        match width {
            1 => self.ram[off] = value as u8,
            2 => self.ram[off..off+2].copy_from_slice(&(value as u16).to_le_bytes()),
            4 => self.ram[off..off+4].copy_from_slice(&(value as u32).to_le_bytes()),
            8 => self.ram[off..off+8].copy_from_slice(&value.to_le_bytes()),
            _ => unreachable!("invalid store width {width}"),
        }
        Ok(())
    }

    fn offset(&self, addr: u64, width: usize) -> Result<usize> {
        let end = addr.wrapping_add(width as u64);
        let ram_end = self.ram_base.wrapping_add(self.ram.len() as u64);
        if addr < self.ram_base || end > ram_end {
            return Err(anyhow!("bus fault: addr={addr:#x} width={width}"));
        }
        Ok((addr - self.ram_base) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> Bus { Bus::new(64, 0x8000_0000) }

    #[test] fn load_store_u8() {
        let mut b = bus();
        b.store(0x8000_0000, 1, 0xAB).unwrap();
        assert_eq!(b.load(0x8000_0000, 1).unwrap(), 0xAB);
    }

    #[test] fn load_store_u16_le() {
        let mut b = bus();
        b.store(0x8000_0000, 2, 0x1234).unwrap();
        assert_eq!(b.load(0x8000_0000, 1).unwrap(), 0x34); // low byte first
        assert_eq!(b.load(0x8000_0001, 1).unwrap(), 0x12);
    }

    #[test] fn load_store_u64() {
        let mut b = bus();
        b.store(0x8000_0000, 8, 0xDEADBEEF_CAFEBABE).unwrap();
        assert_eq!(b.load(0x8000_0000, 8).unwrap(), 0xDEADBEEF_CAFEBABE);
    }

    #[test] fn out_of_bounds_returns_err() {
        let b = bus();
        assert!(b.load(0x0000_0000, 4).is_err());
        assert!(b.load(0x8000_0040, 4).is_err()); // past end
    }
}
```

- [ ] **Run unit tests**

```bash
cargo test bus
```

Expected: 4 tests pass.

- [ ] **Commit**

```bash
git add src/bus.rs
git commit -m "feat: Bus struct with little-endian RAM load/store"
```

---

## Task 3: Cpu struct and Tracer

**Files:**
- Modify: `src/cpu/mod.rs`

- [ ] **Replace `src/cpu/mod.rs`**

```rust
pub mod decode;
pub mod execute;

use crate::bus::Bus;
use anyhow::Result;

pub struct Tracer {
    enabled: bool,
}

impl Tracer {
    pub fn new(enabled: bool) -> Self { Self { enabled } }

    pub fn trace_step(&self, pc: u64, raw: u32, mnemonic: &str, operands: &str,
                      reg_changes: &[(usize, u64, u64)]) {
        if !self.enabled { return; }
        let changes: String = reg_changes.iter()
            .map(|(r, old, new)| format!(" x{r}: {old:#018x} -> {new:#018x}"))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!("[{pc:#010x}] {raw:08x}  {mnemonic:<8} {operands:<24}{changes}");
    }
}

pub struct Cpu {
    regs:   [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
}

impl Cpu {
    pub fn new(bus: Bus, entry: u64, trace: bool) -> Self {
        Self { regs: [0u64; 32], pc: entry, bus, tracer: Tracer::new(trace) }
    }

    /// Read register. x0 always returns 0.
    #[inline(always)]
    pub fn reg(&self, n: usize) -> u64 {
        if n == 0 { 0 } else { self.regs[n] }
    }

    /// Write register. Writes to x0 are silently ignored.
    #[inline(always)]
    pub fn set_reg(&mut self, n: usize, val: u64) {
        if n != 0 { self.regs[n] = val; }
    }

    /// Fetch, decode, execute one instruction. Advances pc.
    pub fn step(&mut self) -> Result<()> {
        use decode::decode;
        use execute::execute;
        let raw = self.bus.load(self.pc, 4)? as u32;
        let inst = decode(raw)?;
        execute(self, inst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;

    fn cpu() -> Cpu {
        Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false)
    }

    #[test] fn x0_always_zero() {
        let mut c = cpu();
        c.set_reg(0, 0xDEAD);
        assert_eq!(c.reg(0), 0);
    }

    #[test] fn reg_read_write() {
        let mut c = cpu();
        c.set_reg(5, 0xCAFE);
        assert_eq!(c.reg(5), 0xCAFE);
    }
}
```

- [ ] **Run unit tests**

```bash
cargo test cpu
```

Expected: 2 tests pass.

- [ ] **Commit**

```bash
git add src/cpu/mod.rs
git commit -m "feat: Cpu struct with reg accessors and Tracer"
```

---

## Task 4: Instruction enum

**Files:**
- Modify: `src/cpu/decode.rs`

- [ ] **Replace `src/cpu/decode.rs` with the full enum** (decode() stub only — implemented in Task 5)

```rust
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
```

- [ ] **Verify it compiles**

```bash
cargo build
```

Expected: compiles, no errors.

- [ ] **Commit**

```bash
git add src/cpu/decode.rs
git commit -m "feat: Instruction enum for all RV64I instructions"
```

---

## Task 5: decode() function

**Files:**
- Modify: `src/cpu/decode.rs`

- [ ] **Write unit tests for decode** (add at the bottom of `src/cpu/decode.rs`)

```rust
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

    // LUI x1, 0x12345   →  0x12345537  (imm field = upper 20 bits already shifted)
    // LUI: imm = SignExt(inst[31:12] << 12) = 0x12345000
    #[test] fn decode_lui() {
        let inst = decode(0x123450b7).unwrap();
        assert_eq!(inst, Instruction::Lui { rd: 1, imm: 0x12345000 });
    }

    // SRAI x1, x1, 3   →  0x40305093
    #[test] fn decode_srai() {
        let inst = decode(0x40305093).unwrap();
        assert_eq!(inst, Instruction::Srai { rd: 1, rs1: 1, shamt: 3 });
    }

    // ADDIW x1, x1, -1   →  0xfff0809b
    #[test] fn decode_addiw() {
        let inst = decode(0xfff0809b).unwrap();
        assert_eq!(inst, Instruction::Addiw { rd: 1, rs1: 1, imm: -1 });
    }
}
```

- [ ] **Run tests — they should fail**

```bash
cargo test decode
```

Expected: all decode tests fail (decode returns Err).

- [ ] **Implement the immediate extraction helpers** (private, top of the impl section in `decode.rs`, above `pub fn decode`)

```rust
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

/// Extract register index from bit range [hi:lo].
fn reg(inst: u32, lo: u32) -> usize { ((inst >> lo) & 0x1f) as usize }
```

- [ ] **Implement `pub fn decode(inst: u32) -> Result<Instruction>`**

```rust
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
            _ => Err(anyhow!("illegal R-type funct3={funct3:#x} funct7={funct7:#x}")),
        },
        // RV64I W-variants R-type: opcode 0x3B
        0x3B => match (funct3, funct7) {
            (0x0, 0x00) => Ok(Instruction::Addw { rd, rs1, rs2 }),
            (0x0, 0x20) => Ok(Instruction::Subw { rd, rs1, rs2 }),
            (0x1, 0x00) => Ok(Instruction::Sllw { rd, rs1, rs2 }),
            (0x5, 0x00) => Ok(Instruction::Srlw { rd, rs1, rs2 }),
            (0x5, 0x20) => Ok(Instruction::Sraw { rd, rs1, rs2 }),
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
        0x73 => match i_imm(inst) {
            0 => Ok(Instruction::Ecall),
            1 => Ok(Instruction::Ebreak),
            _ => Err(anyhow!("illegal system instruction imm={:#x}", i_imm(inst))),
        },
        _ => Err(anyhow!("illegal opcode {opcode:#x} at inst={inst:#010x}")),
    }
}
```

- [ ] **Run decode tests**

```bash
cargo test decode
```

Expected: all tests pass. If `decode_add` fails, re-check the bit layout: rd=inst[11:7], rs1=inst[19:15], rs2=inst[24:20].

- [ ] **Commit**

```bash
git add src/cpu/decode.rs
git commit -m "feat: decode() for all RV64I instructions"
```

---

## Task 6: execute() function

**Files:**
- Modify: `src/cpu/execute.rs`

- [ ] **Replace `src/cpu/execute.rs`** with the full implementation

```rust
use anyhow::{anyhow, Result};
use crate::cpu::{Cpu, decode::Instruction};

/// Sign-extend a u64 value from bit `n` (0-indexed).
/// Used for *W variants: sext(val, 31) sign-extends from bit 31.
#[inline(always)]
fn sext(val: u64, bit: u32) -> u64 {
    let shift = 63 - bit;
    (((val << shift) as i64) >> shift) as u64
}

pub fn execute(cpu: &mut Cpu, inst: Instruction) -> Result<()> {
    let pc = cpu.pc;

    // Most instructions advance pc by 4. Branches and jumps set pc directly.
    let mut next_pc = pc.wrapping_add(4);

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
        // All sign-extend except LBU/LHU/LWU. Spec: Unprivileged §2.6
        Instruction::Lb  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = sext(cpu.bus.load(addr, 1)?, 7);
            cpu.set_reg(rd, v);
        },
        Instruction::Lh  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = sext(cpu.bus.load(addr, 2)?, 15);
            cpu.set_reg(rd, v);
        },
        Instruction::Lw  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = sext(cpu.bus.load(addr, 4)?, 31);
            cpu.set_reg(rd, v);
        },
        Instruction::Ld  { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            let v = cpu.bus.load(addr, 8)?;
            cpu.set_reg(rd, v);
        },
        Instruction::Lbu { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            cpu.set_reg(rd, cpu.bus.load(addr, 1)?);
        },
        Instruction::Lhu { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            cpu.set_reg(rd, cpu.bus.load(addr, 2)?);
        },
        Instruction::Lwu { rd, rs1, imm } => {
            let addr = cpu.reg(rs1).wrapping_add(imm as u64);
            cpu.set_reg(rd, cpu.bus.load(addr, 4)?); // zero-extended (load already gives u64)
        },

        // --- Stores ---
        Instruction::Sb { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 1, cpu.reg(rs2))?;
        },
        Instruction::Sh { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 2, cpu.reg(rs2))?;
        },
        Instruction::Sw { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 4, cpu.reg(rs2))?;
        },
        Instruction::Sd { rs1, rs2, imm } => {
            cpu.bus.store(cpu.reg(rs1).wrapping_add(imm as u64), 8, cpu.reg(rs2))?;
        },

        // --- Branches ---
        Instruction::Beq  { rs1, rs2, imm } => {
            if cpu.reg(rs1) == cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bne  { rs1, rs2, imm } => {
            if cpu.reg(rs1) != cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Blt  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) < (cpu.reg(rs2) as i64) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bge  { rs1, rs2, imm } => {
            if (cpu.reg(rs1) as i64) >= (cpu.reg(rs2) as i64) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bltu { rs1, rs2, imm } => {
            if cpu.reg(rs1) < cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },
        Instruction::Bgeu { rs1, rs2, imm } => {
            if cpu.reg(rs1) >= cpu.reg(rs2) { next_pc = pc.wrapping_add(imm as u64); }
        },

        // --- Jumps ---
        // JAL: rd = pc+4, pc = pc + imm. Spec: Unprivileged §2.5
        Instruction::Jal { rd, imm } => {
            cpu.set_reg(rd, next_pc);
            next_pc = pc.wrapping_add(imm as u64);
        },
        // JALR: rd = pc+4, pc = (rs1 + imm) & ~1. Spec: Unprivileged §2.5
        Instruction::Jalr { rd, rs1, imm } => {
            let target = cpu.reg(rs1).wrapping_add(imm as u64) & !1;
            cpu.set_reg(rd, next_pc);
            next_pc = target;
        },

        // --- Upper immediate ---
        // LUI: rd = SignExt(imm[31:12] << 12). imm already has lower 12 bits = 0.
        Instruction::Lui   { rd, imm } => { cpu.set_reg(rd, imm as u64); },
        // AUIPC: rd = pc + SignExt(imm[31:12] << 12).
        Instruction::Auipc { rd, imm } => { cpu.set_reg(rd, pc.wrapping_add(imm as u64)); },

        // --- System ---
        Instruction::Fence  => { /* NOP in Phase 1 — no memory-ordering concerns */ },
        Instruction::Ecall  => return Err(anyhow!("ecall at pc={pc:#x}")),
        Instruction::Ebreak => return Err(anyhow!("ebreak at pc={pc:#x}")),
    }

    cpu.pc = next_pc;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bus::Bus, cpu::{Cpu, decode::{decode, Instruction}}};

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
}
```

- [ ] **Run execute unit tests**

```bash
cargo test execute
```

Expected: all tests pass.

- [ ] **Commit**

```bash
git add src/cpu/execute.rs
git commit -m "feat: execute() for all RV64I instructions"
```

---

## Task 7: ELF loader

**Files:**
- Modify: `src/loader.rs`

- [ ] **Replace `src/loader.rs`**

```rust
use anyhow::{anyhow, Context, Result};
use goblin::elf::{Elf, program_header::PT_LOAD};
use crate::bus::Bus;

pub struct LoadedElf {
    pub bus:        Bus,
    pub entry:      u64,
    pub tohost_addr: Option<u64>,
}

const RAM_BASE: u64 = 0x8000_0000;
const RAM_SIZE: usize = 128 * 1024 * 1024; // 128 MiB — enough for test ELFs

/// Load an ELF64 binary: copy PT_LOAD segments into RAM, return entry point
/// and (if present) the address of the `tohost` symbol used by riscv-tests.
pub fn load_elf(bytes: &[u8]) -> Result<LoadedElf> {
    let elf = Elf::parse(bytes).context("ELF parse failed")?;
    if elf.is_64 == false {
        return Err(anyhow!("expected ELF64, got ELF32"));
    }

    let mut bus = Bus::new(RAM_SIZE, RAM_BASE);

    for ph in &elf.program_headers {
        if ph.p_type != PT_LOAD { continue; }
        let file_start = ph.p_offset as usize;
        let file_end   = file_start + ph.p_filesz as usize;
        let mem_addr   = ph.p_paddr;

        if mem_addr < RAM_BASE {
            return Err(anyhow!("PT_LOAD segment at {mem_addr:#x} is below RAM base {RAM_BASE:#x}"));
        }

        let segment = bytes.get(file_start..file_end)
            .ok_or_else(|| anyhow!("PT_LOAD segment out of file bounds"))?;

        let off = (mem_addr - RAM_BASE) as usize;
        bus.ram_mut()[off..off + segment.len()].copy_from_slice(segment);
    }

    let tohost_addr = elf.syms.iter()
        .find(|s| elf.strtab.get_at(s.st_name) == Some("tohost"))
        .map(|s| s.st_value);

    Ok(LoadedElf { bus, entry: elf.entry, tohost_addr })
}
```

- [ ] **Run `cargo build` to verify it compiles**

```bash
cargo build
```

Expected: compiles without errors.

- [ ] **Commit**

```bash
git add src/loader.rs
git commit -m "feat: ELF64 loader with PT_LOAD and tohost symbol lookup"
```

---

## Task 8: main.rs and Tracer integration

**Files:**
- Modify: `src/main.rs`
- Modify: `src/cpu/mod.rs` (wire Tracer into step)

- [ ] **Replace `src/main.rs`**

```rust
use anyhow::{Context, Result};
use clap::Parser;
use riscv_emu::{cpu::Cpu, loader};

#[derive(Parser)]
#[command(name = "riscv-emu", about = "RV64I emulator")]
struct Args {
    /// ELF binary to run
    binary: String,

    /// Enable per-instruction trace output on stderr
    #[arg(long)]
    trace: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let bytes = std::fs::read(&args.binary)
        .with_context(|| format!("failed to read {}", args.binary))?;

    let loaded = loader::load_elf(&bytes)?;
    let mut cpu = Cpu::new(loaded.bus, loaded.entry, args.trace);

    loop {
        cpu.step()?;
    }
}
```

- [ ] **Wire Tracer into `step()` in `src/cpu/mod.rs`**

Replace the `step` method body:

```rust
pub fn step(&mut self) -> Result<()> {
    use decode::decode;
    use execute::execute;

    let raw = self.bus.load(self.pc, 4)? as u32;
    let inst = decode(raw)?;

    if self.tracer.enabled {
        // Snapshot regs before execute to compute diffs
        let before = self.regs;
        let pc = self.pc;
        execute(self, inst)?;
        let changes: Vec<(usize, u64, u64)> = (1..32)
            .filter(|&i| before[i] != self.regs[i])
            .map(|i| (i, before[i], self.regs[i]))
            .collect();
        // Re-decode for mnemonic (cheap)
        let inst2 = decode(raw).unwrap();
        let mnemonic = format!("{inst2:?}");
        let short = mnemonic.split(' ').next().unwrap_or(&mnemonic);
        self.tracer.trace_step(pc, raw, short, "", &changes);
    } else {
        execute(self, inst)?;
    }

    Ok(())
}
```

- [ ] **Verify binary runs** (it will loop forever on invalid ELF, but should compile and link)

```bash
cargo build --release
```

Expected: compiles cleanly.

- [ ] **Commit**

```bash
git add src/main.rs src/cpu/mod.rs
git commit -m "feat: CLI entry point with --trace flag"
```

---

## Task 9: riscv-tests harness

**Files:**
- Create: `tests/riscv-tests/.gitkeep`
- Create: `scripts/fetch-riscv-tests.sh`
- Modify: `build.rs`
- Create: `tests/riscv_tests.rs`

- [ ] **Create `tests/riscv-tests/.gitkeep`** (directory placeholder)

```bash
mkdir -p tests/riscv-tests
touch tests/riscv-tests/.gitkeep
```

- [ ] **Create `scripts/fetch-riscv-tests.sh`**

This script builds the test ELFs using the system's RISC-V toolchain. Run once, commit the results.

```bash
#!/usr/bin/env bash
# Builds rv64ui-p-* test ELFs from source and copies them to tests/riscv-tests/.
# Requires: riscv64-unknown-elf-gcc (or riscv64-linux-gnu-gcc), autoconf, make.
# On Ubuntu/Debian: sudo apt install gcc-riscv64-unknown-elf autoconf
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="$SCRIPT_DIR/../tests/riscv-tests"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Cloning riscv-tests..."
git clone --depth 1 https://github.com/riscv-software-src/riscv-tests "$WORK/riscv-tests"
cd "$WORK/riscv-tests"
git submodule update --init --recursive

echo "Configuring..."
autoconf
./configure --prefix="$WORK/install"

echo "Building isa tests..."
make isa -j"$(nproc)"

echo "Copying rv64ui-p-* to $DEST..."
mkdir -p "$DEST"
# Exclude .dump files (disassembly listings) and .o files
find "$WORK/riscv-tests/isa" -name 'rv64ui-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Done. $(ls "$DEST" | grep -c rv64ui-p-) test ELFs copied."
echo "Now run: git add tests/riscv-tests && git commit -m 'test: vendor rv64ui-p-* ELFs'"
```

```bash
chmod +x scripts/fetch-riscv-tests.sh
```

- [ ] **Replace `build.rs`** with test generator

```rust
use std::{env, fs, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let tests_dir = Path::new("tests/riscv-tests");

    let mut code = String::from(
        "fn run_riscv_test(path: &std::path::Path) {\n\
            let bytes = std::fs::read(path).expect(\"read test ELF\");\n\
            let loaded = riscv_emu::loader::load_elf(&bytes).expect(\"load ELF\");\n\
            let tohost = loaded.tohost_addr.expect(\"no tohost symbol\");\n\
            let mut cpu = riscv_emu::cpu::Cpu::new(loaded.bus, loaded.entry, false);\n\
            for _ in 0..100_000_000u64 {\n\
                match cpu.step() {\n\
                    Ok(()) => {}\n\
                    Err(e) => panic!(\"cpu error: {e}\"),\n\
                }\n\
                if let Ok(v) = cpu.bus.load(tohost, 8) {\n\
                    if v != 0 {\n\
                        assert_eq!(v, 1, \"test failed: tohost={v:#x} file={}\", path.display());\n\
                        return;\n\
                    }\n\
                }\n\
            }\n\
            panic!(\"test timed out: {}\", path.display());\n\
        }\n\n"
    );

    let mut found = false;
    if let Ok(entries) = fs::read_dir(tests_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if !name.starts_with("rv64ui-p-") || name.ends_with(".dump") || name == ".gitkeep" {
                continue;
            }
            found = true;
            let fn_name = name.replace('-', "_");
            let abs_path = path.canonicalize().unwrap();
            code.push_str(&format!(
                "#[test]\nfn {fn_name}() {{\n    \
                    run_riscv_test(std::path::Path::new({:?}));\n}}\n\n",
                abs_path.to_str().unwrap()
            ));
        }
    }

    if !found {
        code.push_str(
            "#[test]\nfn _no_riscv_tests_vendored() {\n    \
                panic!(\"No rv64ui-p-* ELFs found in tests/riscv-tests/. \
                Run scripts/fetch-riscv-tests.sh first.\");\n}\n"
        );
    }

    fs::write(Path::new(&out_dir).join("riscv_tests_generated.rs"), code).unwrap();
    println!("cargo:rerun-if-changed=tests/riscv-tests");
}
```

- [ ] **Create `tests/riscv_tests.rs`**

```rust
// Generated tests are included here. Each calls run_riscv_test() defined in build.rs output.
include!(concat!(env!("OUT_DIR"), "/riscv_tests_generated.rs"));
```

- [ ] **Verify it compiles** (the sentinel test will be generated)

```bash
cargo test _no_riscv_tests_vendored -- --nocapture
```

Expected: test runs and panics with the "run fetch script" message — that's correct.

- [ ] **Commit**

```bash
git add build.rs tests/riscv_tests.rs tests/riscv-tests/.gitkeep scripts/fetch-riscv-tests.sh
git commit -m "test: riscv-tests harness with build.rs code generation"
```

---

## Task 10: Vendor ELFs and pass all rv64ui-p-* tests

**Files:**
- Create: `tests/riscv-tests/rv64ui-p-*` (one file per test, ~40 files)

- [ ] **Check if `riscv64-unknown-elf-gcc` is available**

```bash
riscv64-unknown-elf-gcc --version 2>/dev/null || riscv64-linux-gnu-gcc --version 2>/dev/null || echo "not found"
```

If not found, install it:
```bash
sudo apt install gcc-riscv64-unknown-elf autoconf make
```

- [ ] **Run the fetch script**

```bash
bash scripts/fetch-riscv-tests.sh
```

Expected: ~40 `rv64ui-p-*` ELF files appear in `tests/riscv-tests/`.

- [ ] **Run all tests**

```bash
cargo test rv64ui_p_ -- --test-threads=1
```

Expect most tests to pass. Any failures indicate a bug in decode or execute for that instruction. Debug with `--trace`:

```bash
./target/debug/riscv-emu --trace tests/riscv-tests/rv64ui-p-add 2>&1 | head -50
```

- [ ] **Fix any failing tests**

Common bugs to check:
- **Sign extension on loads (LB/LH/LW):** `sext(val, 7/15/31)` — verify the bit position.
- **`*W` sign extension:** must sign-extend from bit 31, not 63.
- **Branch immediates:** B-type has a non-obvious encoding. Verify `b_imm()` with a known encoding.
- **JALR clears bit 0:** `target & !1` — easy to forget.
- **SLTIU:** compares unsigned but the immediate is sign-extended first (then treated as unsigned). `cpu.reg(rs1) < (imm as u64)` — since `imm` is `i64`, `imm as u64` gives the correct 64-bit unsigned value.

- [ ] **When all tests pass, commit the vendored ELFs**

```bash
git add tests/riscv-tests/
git commit -m "test: vendor rv64ui-p-* ELFs, all tests passing"
```

- [ ] **Final check**

```bash
cargo test
```

Expected: all `rv64ui_p_*` tests pass, zero failures.
