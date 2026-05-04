# RISC-V Emulator Phase 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pass all `rv64si-p-*` and `rv64ui-v-*` riscv-tests by adding S-mode supervisor privilege and an Sv39 virtual memory unit with a 64-entry direct-mapped TLB.

**Architecture:** Two sequential task groups — S-mode privilege first (PrivMode enum, medeleg/mideleg, sstatus/sie/sip filtered views, trap delegation, SRET/ECALL updates), then Sv39 MMU (new `src/cpu/mmu.rs`, integrate into Cpu, update all memory ops). Each group ends with a test gate.

**Tech Stack:** Rust stable, existing `crate::bus::Bus` (physical load/store), `anyhow` for error propagation.

---

## File map

| File | Change |
|------|--------|
| `src/cpu/mod.rs` | Add `PrivMode` enum + `pub mod mmu`; add `mode`/`mmu` fields to `Cpu`; update `deliver_trap()` for delegation; update `step()` for MMU fetch |
| `src/cpu/csr.rs` | Add `medeleg`/`mideleg` fields; add sstatus/sie/sip/scounteren read+write; add `s_trap_entry()`/`s_ret()` helpers |
| `src/cpu/execute.rs` | Update MRET (restore mode), SRET (full privilege restore), ECALL (mode-aware cause), EBREAK (use deliver_trap); route all 8 load+store+AMO ops through `mmu.translate()` |
| `src/cpu/decode.rs` | Add `SfenceVma` variant |
| `src/cpu/mmu.rs` | **New.** `Mmu` struct, 64-entry direct-mapped TLB, Sv39 3-level walk, A/D hardware updates, permission checks |
| `build.rs` | Add `rv64si-p-*` and `rv64ui-v-*` patterns |
| `scripts/fetch-riscv-tests.sh` | Add those two suites to copy step |

---

## Group A: S-mode Privilege

---

### Task 1: Add PrivMode + S-mode CSR extensions

**Files:**
- Modify: `src/cpu/mod.rs` (add PrivMode enum, pub mod mmu)
- Modify: `src/cpu/csr.rs` (new fields, read/write arms, helpers)

- [ ] **Step 1: Add `PrivMode` to `src/cpu/mod.rs`**

Insert immediately after the `use` imports, before `pub struct Tracer`:

```rust
pub mod mmu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivMode { U = 0, S = 1, M = 3 }
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build 2>&1 | head -20
```

Expected: "missing file mmu.rs" or similar — that's fine. Create an empty placeholder:

```bash
touch src/cpu/mmu.rs
```

Then: `cargo build` should succeed.

- [ ] **Step 3: Add `medeleg`/`mideleg` fields to `Csr` struct in `src/cpu/csr.rs`**

Add to the struct after `mip`:
```rust
pub medeleg: u64,
pub mideleg: u64,
```

Add to `new()` initializer after `mip: 0,`:
```rust
medeleg: 0,
mideleg: 0,
```

- [ ] **Step 4: Add sstatus/sie/sip/scounteren/medeleg/mideleg to `Csr::read()`**

Add these arms to the `match addr` in `read()`:

```rust
// sstatus: S-mode view of mstatus. UXL[33:32] hardwired 2.
// SSTATUS_MASK covers: SD[63], UXL[33:32], MXR[19], SUM[18], XS[16:15],
//   FS[14:13], SPP[8], UBE[6], SPIE[5], SIE[1]. Priv §4.1.1
0x100 => (self.mstatus | 0x0000_0002_0000_0000) & 0x8000_0003_000D_E162,
0x104 => self.mie  & 0x222,  // sie: SSIE[1], STIE[5], SEIE[9]. Priv §4.1.3
0x106 => 0,                  // scounteren: stub reads 0. Priv §4.1.5
0x144 => self.mip  & 0x222,  // sip: S-mode view of mip. Priv §4.1.4
0x302 => self.medeleg,       // medeleg. Priv §3.1.8
0x303 => self.mideleg,       // mideleg. Priv §3.1.8
```

- [ ] **Step 5: Add sstatus/sie/sip/medeleg/mideleg to `Csr::write()`**

```rust
// sstatus write: only update S-mode writable bits in mstatus.
// Writable SSTATUS bits (no SD, no UXL): 0x0000_0000_000D_E162
0x100 => self.mstatus = (self.mstatus & !0x0000_0000_000D_E162)
                        | (val         &  0x0000_0000_000D_E162),
0x104 => self.mie  = (self.mie  & !0x222) | (val & 0x222),
0x106 => {}                 // scounteren: writes ignored
0x144 => self.mip  = (self.mip  & !0x222) | (val & 0x222),
0x302 => self.medeleg = val,
0x303 => self.mideleg = val,
```

- [ ] **Step 6: Add `s_trap_entry()` and `s_ret()` to `Csr` impl**

```rust
/// Called on trap delivery to S-mode. Saves SIE into SPIE; sets SPP to prior mode.
/// from_smode: true if the trap was taken from S-mode (SPP=1), false if from U-mode (SPP=0).
pub fn s_trap_entry(&mut self, from_smode: bool) {
    let sie = (self.mstatus >> 1) & 1;
    self.mstatus = (self.mstatus & !0x122u64) // clear SIE[1], SPIE[5], SPP[8]
        | (sie << 5)                           // SPIE ← SIE
        | ((from_smode as u64) << 8);          // SPP ← 1 if from S, 0 if from U
}

/// Called by SRET. Restores SIE←SPIE, sets SPIE←1, clears SPP to U.
pub fn s_ret(&mut self) {
    let spie = (self.mstatus >> 5) & 1;
    self.mstatus = (self.mstatus & !0x122u64) // clear SIE[1], SPIE[5], SPP[8]
        | (spie << 1)                          // SIE ← SPIE
        | (1u64 << 5);                         // SPIE ← 1, SPP ← 0 (U)
}
```

- [ ] **Step 7: Write unit tests in `src/cpu/csr.rs`**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn s_trap_entry_from_u_mode() {
    let mut csr = Csr::new();
    csr.mstatus = 0b0000_0010; // SIE=1 (bit 1)
    csr.s_trap_entry(false);   // from U-mode → SPP=0
    assert_eq!((csr.mstatus >> 1) & 1,  0); // SIE cleared
    assert_eq!((csr.mstatus >> 5) & 1,  1); // SPIE = old SIE
    assert_eq!((csr.mstatus >> 8) & 1,  0); // SPP = U-mode
}

#[test]
fn s_trap_entry_from_s_mode() {
    let mut csr = Csr::new();
    csr.mstatus = 0b0000_0010; // SIE=1
    csr.s_trap_entry(true);    // from S-mode → SPP=1
    assert_eq!((csr.mstatus >> 8) & 1, 1); // SPP = S-mode
}

#[test]
fn s_ret_restores_sie() {
    let mut csr = Csr::new();
    csr.mstatus = 0b0010_0000; // SPIE=1 (bit 5)
    csr.s_ret();
    assert_eq!((csr.mstatus >> 1) & 1, 1); // SIE ← SPIE=1
    assert_eq!((csr.mstatus >> 5) & 1, 1); // SPIE ← 1
    assert_eq!((csr.mstatus >> 8) & 1, 0); // SPP ← 0 (U)
}

#[test]
fn sstatus_filters_mstatus() {
    let mut csr = Csr::new();
    // SIE=1 in mstatus
    csr.mstatus = 0x2;
    let sstatus = csr.read(0x100);
    assert_eq!(sstatus & 0x2, 0x2);                  // SIE visible
    assert_eq!((sstatus >> 32) & 3, 2);              // UXL=2 hardwired
    // M-mode bits not visible: MIE (bit 3) must be masked
    csr.mstatus = 0x8; // MIE=1
    assert_eq!(csr.read(0x100) & 0x8, 0);            // MIE hidden in sstatus
}

#[test]
fn sie_filters_mie() {
    let mut csr = Csr::new();
    csr.mie = 0xFFFF_FFFF;
    assert_eq!(csr.read(0x104), 0x222); // only SSIE/STIE/SEIE visible
    csr.write(0x104, 0xFFFF);
    assert_eq!(csr.mie & !0x222, 0xFFFF_FFFF & !0x222); // M-mode bits unchanged
    assert_eq!(csr.mie & 0x222, 0x222);                  // S-mode bits updated
}

#[test]
fn medeleg_round_trips() {
    let mut csr = Csr::new();
    csr.write(0x302, 0xB109);
    assert_eq!(csr.read(0x302), 0xB109);
}
```

- [ ] **Step 8: Run tests**

```bash
cargo test csr
```

Expected: all csr tests pass (existing + new 6).

- [ ] **Step 9: Commit**

```bash
git add src/cpu/mod.rs src/cpu/mmu.rs src/cpu/csr.rs
git commit -m "feat: add PrivMode enum + S-mode CSR extensions (medeleg, sstatus/sie/sip views, s_trap helpers)"
```

---

### Task 2: Add `mode` field to `Cpu` + update `deliver_trap()` for delegation

**Files:**
- Modify: `src/cpu/mod.rs`

- [ ] **Step 1: Add `mode` field to `Cpu` struct**

```rust
pub struct Cpu {
    regs:   [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
    pub csr: Csr,
    pub reservation: Option<u64>,
    pub mode: PrivMode,        // ← add this
}
```

In `Cpu::new()`, add `mode: PrivMode::M,` to the struct literal.

- [ ] **Step 2: Update `deliver_trap()` to check medeleg**

Replace the existing `deliver_trap` body:

```rust
pub fn deliver_trap(&mut self, cause: u64, tval: u64) {
    // Delegate to S-mode if medeleg has this exception bit set and we're not in M-mode.
    // Priv §3.1.8
    if cause < 64 && (self.csr.medeleg >> cause) & 1 == 1 && self.mode != PrivMode::M {
        self.csr.s_trap_entry(self.mode == PrivMode::S);
        self.csr.sepc   = self.pc;
        self.csr.scause = cause;
        self.csr.stval  = tval;
        self.pc  = self.csr.stvec & !0b11;
        self.mode = PrivMode::S;
    } else {
        self.csr.trap_entry();
        self.csr.mepc   = self.pc;
        self.csr.mcause = cause;
        self.csr.mtval  = tval;
        self.pc  = self.csr.mtvec & !0b11;
        self.mode = PrivMode::M;
    }
}
```

- [ ] **Step 3: Write unit tests**

Add to `#[cfg(test)] mod tests` in `src/cpu/mod.rs`:

```rust
#[test]
fn deliver_trap_routes_to_m_when_no_delegation() {
    let mut c = cpu();
    c.csr.mtvec = 0x8000_0100;
    c.csr.medeleg = 0; // nothing delegated
    c.deliver_trap(8, 0);
    assert_eq!(c.csr.mcause, 8);
    assert_eq!(c.pc, 0x8000_0100);
    assert_eq!(c.mode, PrivMode::M);
}

#[test]
fn deliver_trap_routes_to_s_when_delegated() {
    let mut c = cpu();
    c.csr.stvec  = 0x8000_0200;
    c.csr.mtvec  = 0x8000_0100;
    c.csr.medeleg = 1 << 8; // delegate cause 8 (ecall from U-mode)
    c.mode = PrivMode::U;
    c.deliver_trap(8, 0xABCD);
    assert_eq!(c.csr.scause, 8);
    assert_eq!(c.csr.stval,  0xABCD);
    assert_eq!(c.pc,   0x8000_0200);
    assert_eq!(c.mode, PrivMode::S);
    // M-mode registers untouched
    assert_eq!(c.csr.mcause, 0);
}

#[test]
fn deliver_trap_m_mode_not_delegated_even_if_medeleg_set() {
    // Traps from M-mode are never delegated (Priv §3.1.8)
    let mut c = cpu();
    c.csr.mtvec   = 0x8000_0100;
    c.csr.medeleg = 1 << 11; // bit 11 = ecall from M-mode
    c.mode = PrivMode::M;
    c.deliver_trap(11, 0);
    assert_eq!(c.csr.mcause, 11);
    assert_eq!(c.pc, 0x8000_0100);
    assert_eq!(c.mode, PrivMode::M);
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test cpu
```

Expected: all cpu tests pass (existing 4 + new 3).

- [ ] **Step 5: Commit**

```bash
git add src/cpu/mod.rs
git commit -m "feat: add Cpu.mode field + delegation-aware deliver_trap()"
```

---

### Task 3: Update MRET, SRET, ECALL, EBREAK in `execute.rs`

**Files:**
- Modify: `src/cpu/execute.rs`

- [ ] **Step 1: Add import for `PrivMode` at top of `execute.rs`**

The existing import line is:
```rust
use crate::cpu::{Cpu, decode::Instruction};
```

Change to:
```rust
use crate::cpu::{Cpu, PrivMode, decode::Instruction};
```

- [ ] **Step 2: Update `Instruction::Mret` to restore privilege level**

Replace the existing MRET arm:
```rust
// MRET: restore privilege to MPP, then jump to mepc. Priv §3.3.2
Instruction::Mret => {
    let mpp = (cpu.csr.mstatus >> 11) & 3;
    cpu.mode = match mpp { 0 => PrivMode::U, 1 => PrivMode::S, _ => PrivMode::M };
    cpu.csr.mret(); // MIE←MPIE, MPIE←1, MPP←U
    next_pc = cpu.csr.mepc;
},
```

- [ ] **Step 3: Update `Instruction::Sret` to restore privilege level**

Replace the existing SRET arm:
```rust
// SRET: restore privilege to SPP, jump to sepc. Illegal when mstatus.TSR=1. Priv §4.1.1
Instruction::Sret => {
    if (cpu.csr.mstatus >> 22) & 1 != 0 {
        cpu.deliver_trap(2, 0x10200073); // TSR=1: illegal instruction
        next_pc = cpu.pc;
    } else {
        let spp = (cpu.csr.mstatus >> 8) & 1;
        cpu.mode = if spp == 1 { PrivMode::S } else { PrivMode::U };
        cpu.csr.s_ret(); // SIE←SPIE, SPIE←1, SPP←0
        next_pc = cpu.csr.sepc;
    }
},
```

- [ ] **Step 4: Update `Instruction::Ecall` to use mode-aware cause + delegation**

Replace the existing ECALL arm:
```rust
// ECALL: cause depends on current privilege mode. Priv §3.3.1 Table 3.6
// deliver_trap() handles medeleg delegation automatically.
Instruction::Ecall => {
    let cause = match cpu.mode { PrivMode::U => 8, PrivMode::S => 9, PrivMode::M => 11 };
    cpu.deliver_trap(cause, 0);
    next_pc = cpu.pc;
},
```

- [ ] **Step 5: Update `Instruction::Ebreak` to use `deliver_trap()`**

Replace the existing EBREAK arm:
```rust
// EBREAK: cause=3 (breakpoint). Uses deliver_trap so medeleg applies. Priv §3.3.1
Instruction::Ebreak => {
    cpu.deliver_trap(3, cpu.pc);
    next_pc = cpu.pc;
},
```

- [ ] **Step 6: Verify it builds and existing tests still pass**

```bash
cargo test
```

Expected: 103 tests pass (same as Phase 2 baseline).

- [ ] **Step 7: Commit**

```bash
git add src/cpu/execute.rs
git commit -m "feat: update MRET/SRET/ECALL/EBREAK for privilege-level-aware semantics"
```

---

### Task 4: Vendor rv64si ELFs + update build infrastructure + run gate

**Files:**
- Modify: `scripts/fetch-riscv-tests.sh`
- Modify: `build.rs`
- Add: `tests/riscv-tests/rv64si-p-*` ELFs

- [ ] **Step 1: Update `scripts/fetch-riscv-tests.sh`**

After the existing `rv64mi` copy block (after the `echo "Copying rv64mi-p-*"` section), add:

```bash
echo "Copying rv64si-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64si-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;
```

At the end in the summary section, add:
```bash
echo "  rv64si: $(ls "$DEST" | grep -c 'rv64si-p-') ELFs"
```

- [ ] **Step 2: Run the fetch script to vendor the ELFs**

```bash
bash scripts/fetch-riscv-tests.sh
```

Expected: copies rv64si-p-* ELFs to `tests/riscv-tests/`. Typical count: 7 ELFs.

- [ ] **Step 3: Update `build.rs` to recognize rv64si-p-* ELFs**

In the `is_test_elf` predicate inside `build.rs`, add:
```rust
|| name.starts_with("rv64si-p-")
```

Full updated predicate:
```rust
let is_test_elf = name.starts_with("rv64ui-p-")
    || name.starts_with("rv64um-p-")
    || name.starts_with("rv64ua-p-")
    || name.starts_with("rv64mi-p-")
    || name.starts_with("rv64si-p-");
```

- [ ] **Step 4: Commit the ELFs + script + build.rs changes**

```bash
git add tests/riscv-tests/rv64si-p-* scripts/fetch-riscv-tests.sh build.rs
git commit -m "test: vendor rv64si-p-* ELFs and wire into test harness"
```

- [ ] **Step 5: Run rv64si gate**

```bash
cargo test rv64si
```

Expected: all rv64si-p-* tests pass. If any fail, use `--trace` to debug:
```bash
cargo run -- --trace tests/riscv-tests/rv64si-p-<failing-test> 2>&1 | head -100
```

Key behaviors to verify:
- `rv64si-p-scall`: ECALL from S-mode → cause=9, delegated based on medeleg
- `rv64si-p-sbreak`: EBREAK from S-mode → cause=3
- `rv64si-p-csr`: sstatus/sie/sip read/write
- `rv64si-p-dirty`: hardware A/D bit updates (needs MMU — this test may fail until Task 6–8)

Note: `rv64si-p-dirty` requires Sv39 MMU and may not pass until Task 8. That's acceptable — run the gate after Task 8 if needed. All other rv64si tests should pass here.

- [ ] **Step 6: Commit any fixes**

```bash
git add src/cpu/
git commit -m "fix: <description of fix>" # if needed
```

---

## Group B: Sv39 MMU

---

### Task 5: Create `src/cpu/mmu.rs` with passthrough + flush

**Files:**
- Modify: `src/cpu/mmu.rs` (was empty placeholder)
- Modify: `src/cpu/mod.rs` (add `mmu: Mmu` field, update `Cpu::new()`)

- [ ] **Step 1: Write unit test for passthrough (TDD)**

Add `#[cfg(test)] mod tests` block to `src/cpu/mmu.rs` (will grow in Task 6):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::cpu::PrivMode;

    fn bus() -> Bus { Bus::new(64, 0x8000_0000) }

    #[test]
    fn passthrough_when_m_mode() {
        let mut mmu = Mmu::new();
        let mut b = bus();
        // mode=M always bypasses translation regardless of satp
        let satp_sv39 = (8u64 << 60) | 0x8_0000; // MODE=8, some PPN
        let result = mmu.translate(&mut b, satp_sv39, PrivMode::M, 0, 0xDEAD_0000, AccessType::Load);
        assert_eq!(result.unwrap(), 0xDEAD_0000);
    }

    #[test]
    fn passthrough_when_satp_mode_zero() {
        let mut mmu = Mmu::new();
        let mut b = bus();
        let satp_bare = 0u64; // MODE=0
        let result = mmu.translate(&mut b, satp_bare, PrivMode::S, 0, 0x8000_0000, AccessType::Load);
        assert_eq!(result.unwrap(), 0x8000_0000);
    }

    #[test]
    fn flush_invalidates_all_entries() {
        let mut mmu = Mmu::new();
        // Manually mark one entry as valid
        mmu.tlb[0].valid = true;
        mmu.tlb[5].valid = true;
        mmu.flush();
        assert!(mmu.tlb.iter().all(|e| !e.valid));
    }
}
```

- [ ] **Step 2: Run to confirm tests fail (TDD)**

```bash
cargo test mmu 2>&1 | head -20
```

Expected: compile error (mmu.rs has no content).

- [ ] **Step 3: Implement `src/cpu/mmu.rs` (structs + passthrough only)**

```rust
use crate::bus::Bus;
use super::PrivMode;

const TLB_SIZE: usize = 64;

pub(super) const PTE_V: u64 = 1 << 0;
pub(super) const PTE_R: u64 = 1 << 1;
pub(super) const PTE_W: u64 = 1 << 2;
pub(super) const PTE_X: u64 = 1 << 3;
pub(super) const PTE_U: u64 = 1 << 4;
pub(super) const PTE_G: u64 = 1 << 5;
pub(super) const PTE_A: u64 = 1 << 6;
pub(super) const PTE_D: u64 = 1 << 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType { Fetch, Load, Store }

pub struct MmuFault { pub cause: u64, pub tval: u64 }

#[derive(Clone, Copy)]
pub(super) struct TlbEntry {
    pub valid:    bool,
    pub vpn:      u64,
    pub ppn:      u64,
    pub perm:     u64,
    pub asid:     u16,
    pub pte_addr: u64,  // physical addr of leaf PTE, for A/D writeback
}

pub struct Mmu {
    pub(super) tlb: [TlbEntry; TLB_SIZE],
}

impl Mmu {
    pub fn new() -> Self {
        const EMPTY: TlbEntry = TlbEntry {
            valid: false, vpn: 0, ppn: 0, perm: 0, asid: 0, pte_addr: 0,
        };
        Self { tlb: [EMPTY; TLB_SIZE] }
    }

    pub fn flush(&mut self) {
        for e in &mut self.tlb { e.valid = false; }
    }

    pub fn translate(
        &mut self,
        _bus:    &mut Bus,
        satp:    u64,
        mode:    PrivMode,
        _mstatus: u64,
        addr:    u64,
        _access: AccessType,
    ) -> Result<u64, MmuFault> {
        if (satp >> 60) != 8 || mode == PrivMode::M {
            return Ok(addr);
        }
        // Full walk implemented in Task 6
        Ok(addr)
    }
}
```

- [ ] **Step 4: Add `mmu: Mmu` to `Cpu` in `src/cpu/mod.rs`**

```rust
pub struct Cpu {
    regs:   [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
    pub csr: Csr,
    pub reservation: Option<u64>,
    pub mode: PrivMode,
    pub mmu: mmu::Mmu,       // ← add
}
```

In `Cpu::new()`, add `mmu: mmu::Mmu::new(),` to the struct literal.

- [ ] **Step 5: Run tests**

```bash
cargo test
```

Expected: 103+ tests pass (passthrough tests now included).

- [ ] **Step 6: Commit**

```bash
git add src/cpu/mmu.rs src/cpu/mod.rs
git commit -m "feat: create src/cpu/mmu.rs with passthrough translate() and 64-entry TLB skeleton"
```

---

### Task 6: Implement full Sv39 page walk in `mmu.rs`

**Files:**
- Modify: `src/cpu/mmu.rs`

- [ ] **Step 1: Write a unit test for a minimal 4K page walk**

Add to the `tests` module in `src/cpu/mmu.rs`:

```rust
#[test]
fn sv39_4k_page_walk() {
    // Build a 1-level (single-entry) page table at physical 0x8000_0000.
    // VA 0x0000_1000 → PA 0x8001_0000 (4K page, R/W/X/U, A/D pre-set).
    let mut b = bus();

    // Root page table at 0x8000_0000.
    // VPN[2] of 0x0000_1000 = (0x0000_1000 >> 30) & 0x1FF = 0.
    // Root PTE[0] → points to next-level table at 0x8000_1000.
    let l2_ppn: u64 = 0x8000_1000 >> 12; // 0x80001
    let l2_pte: u64 = (l2_ppn << 10) | PTE_V; // non-leaf (no R/W/X)
    b.store(0x8000_0000, 8, l2_pte).unwrap();

    // Level-1 table at 0x8000_1000.
    // VPN[1] of 0x0000_1000 = (0x0000_1000 >> 21) & 0x1FF = 0.
    // PTE[0] → points to leaf table at 0x8000_2000.
    let l1_ppn: u64 = 0x8000_2000 >> 12; // 0x80002
    let l1_pte: u64 = (l1_ppn << 10) | PTE_V;
    b.store(0x8000_1000, 8, l1_pte).unwrap();

    // Leaf table at 0x8000_2000.
    // VPN[0] of 0x0000_1000 = (0x0000_1000 >> 12) & 0x1FF = 1.
    let target_ppn: u64 = 0x8001_0000 >> 12; // 0x80010
    let leaf_pte: u64 = (target_ppn << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D;
    b.store(0x8000_2000 + 1 * 8, 8, leaf_pte).unwrap(); // VPN[0]=1 → PTE at offset 8

    let satp: u64 = (8u64 << 60) | (0x8000_0000 >> 12); // MODE=8, PPN of root table
    let mut mmu = Mmu::new();

    // Translate VA 0x0000_1ABC (page offset 0xABC)
    let pa = mmu.translate(&mut b, satp, PrivMode::S, 0, 0x0000_1ABC, AccessType::Load).unwrap();
    assert_eq!(pa, 0x8001_0ABC); // target_ppn << 12 | 0xABC
}

#[test]
fn sv39_bad_va_canonical_check() {
    let mut b = bus();
    let satp: u64 = 8u64 << 60;
    let mut mmu = Mmu::new();
    // Non-canonical VA: bit 38=0 but bits 39+ set
    let bad_va: u64 = 0x0080_0000_0000;
    let result = mmu.translate(&mut b, satp, PrivMode::S, 0, bad_va, AccessType::Load);
    assert_eq!(result.unwrap_err().cause, 13); // load page fault
}

#[test]
fn sv39_tlb_hit_after_first_walk() {
    let mut b = bus();
    // Reuse the page table from sv39_4k_page_walk
    let l2_ppn: u64 = 0x8000_1000 >> 12;
    b.store(0x8000_0000, 8, (l2_ppn << 10) | PTE_V).unwrap();
    let l1_ppn: u64 = 0x8000_2000 >> 12;
    b.store(0x8000_1000, 8, (l1_ppn << 10) | PTE_V).unwrap();
    let target_ppn: u64 = 0x8001_0000 >> 12;
    let leaf_pte: u64 = (target_ppn << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D;
    b.store(0x8000_2000 + 1 * 8, 8, leaf_pte).unwrap();

    let satp: u64 = (8u64 << 60) | (0x8000_0000 >> 12);
    let mut mmu = Mmu::new();

    // First walk fills TLB
    let pa1 = mmu.translate(&mut b, satp, PrivMode::S, 0, 0x0000_1000, AccessType::Load).unwrap();
    // Second access hits TLB
    let pa2 = mmu.translate(&mut b, satp, PrivMode::S, 0, 0x0000_1100, AccessType::Load).unwrap();
    assert_eq!(pa1, 0x8001_0000);
    assert_eq!(pa2, 0x8001_0100); // same page, different offset
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test mmu::tests::sv39 2>&1 | head -30
```

Expected: tests fail (translate() still just returns addr).

- [ ] **Step 3: Implement the full `translate()` in `mmu.rs`**

Replace the existing `translate()` implementation with:

```rust
pub fn translate(
    &mut self,
    bus:     &mut Bus,
    satp:    u64,
    mode:    PrivMode,
    mstatus: u64,
    addr:    u64,
    access:  AccessType,
) -> Result<u64, MmuFault> {
    if (satp >> 60) != 8 || mode == PrivMode::M {
        return Ok(addr);
    }

    let vpn = addr >> 12;
    let page_offset = addr & 0xFFF;
    let asid = ((satp >> 44) & 0xFFFF) as u16;

    // Canonical VA check: bits[63:39] must sign-extend bit[38]. Priv §4.3.1
    let top = (addr as i64) >> 38;
    if top != 0 && top != -1 {
        return Err(MmuFault { cause: pf_cause(access), tval: addr });
    }

    // TLB lookup: direct-mapped by VPN & 63
    let idx = (vpn & 63) as usize;
    {
        let e = &self.tlb[idx];
        if e.valid && e.vpn == vpn && (e.asid == asid || e.perm & PTE_G != 0) {
            check_perms(e.perm, mstatus, mode, access, addr)?;
            // Update A/D if needed
            let mut new_perm = e.perm | PTE_A;
            if access == AccessType::Store { new_perm |= PTE_D; }
            if new_perm != e.perm {
                let new_pte = (e.ppn << 10) | new_perm;
                bus.store(e.pte_addr, 8, new_pte).ok();
                self.tlb[idx].perm = new_perm;
            }
            return Ok((e.ppn << 12) | page_offset);
        }
    }

    // 3-level Sv39 page table walk. Priv §4.3.2.
    let vpn_parts = [(addr >> 30) & 0x1FF, (addr >> 21) & 0x1FF, (addr >> 12) & 0x1FF];
    let mut table_pa = (satp & 0x00FF_FFFF_FFFF) << 12;

    for level in 0usize..3 {
        let pte_addr = table_pa + vpn_parts[level] * 8;
        let pte = bus.load(pte_addr, 8)
            .map_err(|_| MmuFault { cause: af_cause(access), tval: addr })?;

        if pte & PTE_V == 0 || (pte & PTE_W != 0 && pte & PTE_R == 0) {
            return Err(MmuFault { cause: pf_cause(access), tval: addr });
        }

        if pte & (PTE_R | PTE_X) != 0 {
            // Leaf PTE at this level
            let ppn = (pte >> 10) & 0x00FF_FFFF_FFFF;

            // Superpage alignment: lower VPN bits of PPN must be zero. Priv §4.3.2 step 5.
            let rem = 2 - level; // 0 for 4K, 1 for 2MB, 2 for 1GB
            if rem > 0 && ppn & ((1u64 << (rem * 9)) - 1) != 0 {
                return Err(MmuFault { cause: pf_cause(access), tval: addr });
            }

            check_perms(pte & 0xFF, mstatus, mode, access, addr)?;

            // Hardware A/D update. Priv §4.3.2 step 7.
            let mut new_pte = pte | PTE_A;
            if access == AccessType::Store { new_pte |= PTE_D; }
            if new_pte != pte {
                bus.store(pte_addr, 8, new_pte)
                    .map_err(|_| MmuFault { cause: af_cause(access), tval: addr })?;
            }

            // Compute PA (superpage-aware)
            let page_bits = 12 + rem * 9;
            let page_size = 1u64 << page_bits;
            let pa = ((ppn >> (rem * 9)) << page_bits) | (addr & (page_size - 1));

            // Fill TLB for 4K pages only (rem == 0)
            if rem == 0 {
                self.tlb[idx] = TlbEntry {
                    valid: true, vpn, ppn, asid,
                    perm: new_pte & 0xFF,
                    pte_addr,
                };
            }

            return Ok(pa);
        }

        // Non-leaf: descend
        table_pa = ((pte >> 10) & 0x00FF_FFFF_FFFF) << 12;
    }

    Err(MmuFault { cause: pf_cause(access), tval: addr })
}
```

Add helper functions after the `Mmu` impl block:

```rust
fn pf_cause(access: AccessType) -> u64 {
    match access { AccessType::Fetch => 12, AccessType::Load => 13, AccessType::Store => 15 }
}

fn af_cause(access: AccessType) -> u64 {
    match access { AccessType::Fetch => 1, AccessType::Load => 5, AccessType::Store => 7 }
}

fn check_perms(perm: u64, mstatus: u64, mode: PrivMode, access: AccessType, tval: u64) -> Result<(), MmuFault> {
    let fault = MmuFault { cause: pf_cause(access), tval };
    let u_bit = perm & PTE_U != 0;
    match mode {
        PrivMode::U => { if !u_bit { return Err(fault); } }
        PrivMode::S => {
            // S-mode can't access U-pages unless mstatus.SUM=1. Priv §4.3.1.
            if u_bit && (mstatus >> 18) & 1 == 0 { return Err(fault); }
        }
        PrivMode::M => {}
    }
    match access {
        AccessType::Fetch => { if perm & PTE_X == 0 { return Err(fault); } }
        AccessType::Load  => {
            // MXR: execute-only pages readable. Priv §4.1.1
            let mxr = (mstatus >> 19) & 1;
            if perm & PTE_R == 0 && !(mxr != 0 && perm & PTE_X != 0) { return Err(fault); }
        }
        AccessType::Store => { if perm & PTE_W == 0 { return Err(fault); } }
    }
    Ok(())
}
```

- [ ] **Step 4: Run MMU unit tests**

```bash
cargo test mmu
```

Expected: all mmu tests pass (passthrough + sv39_4k_page_walk + canonical + tlb_hit).

- [ ] **Step 5: Commit**

```bash
git add src/cpu/mmu.rs
git commit -m "feat: implement Sv39 3-level page walk with 64-entry direct-mapped TLB and hardware A/D updates"
```

---

### Task 7: Integrate `Mmu` into `step()` — MMU fetch

**Files:**
- Modify: `src/cpu/mod.rs`

- [ ] **Step 1: Add `AccessType` to the imports in `mod.rs`**

The existing top of `mod.rs`:
```rust
pub mod csr;
pub mod decode;
pub mod execute;
pub mod mmu;
```

Add a use for AccessType:
```rust
use mmu::AccessType;
```

- [ ] **Step 2: Replace the instruction fetch in `step()` with MMU-aware fetch**

Replace the current fetch block:
```rust
// Instruction fetch — bus error → mcause=1 (instruction access fault)
let raw = match self.bus.load(pc, 4) {
    Ok(v) => v as u32,
    Err(_) => {
        self.deliver_trap(1, pc);
        return Ok(());
    }
};
```

With:
```rust
// Instruction fetch — translate VA → PA, then bus load. Priv §4.3.
let raw = match self.mmu.translate(
    &mut self.bus, self.csr.satp, self.mode, self.csr.mstatus, pc, AccessType::Fetch
) {
    Err(f) => { self.deliver_trap(f.cause, f.tval); return Ok(()); }
    Ok(pa) => match self.bus.load(pa, 4) {
        Err(_) => { self.deliver_trap(1, pc); return Ok(()); } // access fault
        Ok(v)  => v as u32,
    }
};
```

- [ ] **Step 3: Verify all existing tests still pass**

```bash
cargo test
```

Expected: 103+ tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/cpu/mod.rs
git commit -m "feat: route instruction fetch through Sv39 MMU in step()"
```

---

### Task 8: Update all load/store/AMO ops in `execute.rs` to use MMU

**Files:**
- Modify: `src/cpu/execute.rs`

- [ ] **Step 1: Add `AccessType` and `mmu` imports to `execute.rs`**

Change the top of execute.rs:
```rust
use crate::cpu::{Cpu, PrivMode, decode::Instruction};
use crate::cpu::mmu::AccessType;
```

- [ ] **Step 2: Replace all 7 load instructions**

Pattern: translate VA, on fault deliver trap and set `next_pc = cpu.pc`, on success do bus load with PA.

Replace the `// --- Loads ---` section:

```rust
// --- Loads ---
// All addresses go through the MMU. Page faults deliver a trap and skip set_reg.
// Spec: Unprivileged §2.6, Privileged §4.3
Instruction::Lb  { rd, rs1, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 1) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => cpu.set_reg(rd, sext(v, 7)),
        }
    }
},
Instruction::Lh  { rd, rs1, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 2) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => cpu.set_reg(rd, sext(v, 15)),
        }
    }
},
Instruction::Lw  { rd, rs1, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 4) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => cpu.set_reg(rd, sext(v, 31)),
        }
    }
},
Instruction::Ld  { rd, rs1, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 8) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => cpu.set_reg(rd, v),
        }
    }
},
Instruction::Lbu { rd, rs1, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 1) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => cpu.set_reg(rd, v),
        }
    }
},
Instruction::Lhu { rd, rs1, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 2) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => cpu.set_reg(rd, v),
        }
    }
},
Instruction::Lwu { rd, rs1, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 4) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => cpu.set_reg(rd, v),
        }
    }
},
```

- [ ] **Step 3: Replace all 4 store instructions**

Replace the `// --- Stores ---` section:

```rust
// --- Stores ---
Instruction::Sb { rs1, rs2, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Store) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.store(pa, 1, cpu.reg(rs2)) {
            Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
            Ok(_)  => {}
        }
    }
},
Instruction::Sh { rs1, rs2, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Store) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.store(pa, 2, cpu.reg(rs2)) {
            Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
            Ok(_)  => {}
        }
    }
},
Instruction::Sw { rs1, rs2, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Store) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.store(pa, 4, cpu.reg(rs2)) {
            Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
            Ok(_)  => {}
        }
    }
},
Instruction::Sd { rs1, rs2, imm } => {
    let va = cpu.reg(rs1).wrapping_add(imm as u64);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Store) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.store(pa, 8, cpu.reg(rs2)) {
            Err(_) => { cpu.deliver_trap(7, va); next_pc = cpu.pc; }
            Ok(_)  => {}
        }
    }
},
```

- [ ] **Step 4: Update LrD / LrW / ScD / ScW**

These use physical addresses (reservation tracks VA→PA). Translate once:

```rust
Instruction::LrD { rd, rs1 } => {
    let va = cpu.reg(rs1);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 8) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => { cpu.set_reg(rd, v); cpu.reservation = Some(pa); }
        }
    }
},
Instruction::LrW { rd, rs1 } => {
    let va = cpu.reg(rs1);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Load) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => match cpu.bus.load(pa, 4) {
            Err(_) => { cpu.deliver_trap(5, va); next_pc = cpu.pc; }
            Ok(v)  => { cpu.set_reg(rd, sext(v, 31)); cpu.reservation = Some(pa); }
        }
    }
},
Instruction::ScD { rd, rs1, rs2 } => {
    let va = cpu.reg(rs1);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Store) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => {
            if cpu.reservation == Some(pa) {
                cpu.bus.store(pa, 8, cpu.reg(rs2)).ok();
                cpu.set_reg(rd, 0);
            } else {
                cpu.set_reg(rd, 1);
            }
            cpu.reservation = None;
        }
    }
},
Instruction::ScW { rd, rs1, rs2 } => {
    let va = cpu.reg(rs1);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Store) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => {
            if cpu.reservation == Some(pa) {
                cpu.bus.store(pa, 4, cpu.reg(rs2)).ok();
                cpu.set_reg(rd, 0);
            } else {
                cpu.set_reg(rd, 1);
            }
            cpu.reservation = None;
        }
    }
},
```

- [ ] **Step 5: Update all 20 AMO instructions**

AMOs translate with `AccessType::Store` (cause=15 on fault). The pattern for each:

```rust
Instruction::AmoswapD { rd, rs1, rs2 } => {
    let va = cpu.reg(rs1);
    match cpu.mmu.translate(&mut cpu.bus, cpu.csr.satp, cpu.mode, cpu.csr.mstatus, va, AccessType::Store) {
        Err(f) => { cpu.deliver_trap(f.cause, f.tval); next_pc = cpu.pc; }
        Ok(pa) => {
            let old = cpu.bus.load(pa, 8).unwrap_or(0);
            cpu.bus.store(pa, 8, cpu.reg(rs2)).ok();
            cpu.set_reg(rd, old);
        }
    }
},
```

Apply the same `translate → Ok(pa) → bus.load + bus.store` pattern to all remaining AMO variants (AmoswapW, AmoaddD/W, AmoxorD/W, AmoandD/W, AmoorD/W, AmominD/W, AmomaxD/W, AmominuD/W, AmomaxuD/W). The bus operations inside the `Ok(pa)` arm are identical to the existing implementations — only the outer translate wrapper changes.

- [ ] **Step 6: Build and run full test suite**

```bash
cargo test
```

Expected: 103+ tests pass. Any failures indicate a regression in the load/store refactor — check the changed arms carefully.

- [ ] **Step 7: Commit**

```bash
git add src/cpu/execute.rs
git commit -m "feat: route all load/store/AMO/LR/SC ops through Sv39 MMU with page-fault delivery"
```

---

### Task 9: Add `SfenceVma` to `decode.rs` and `execute.rs`

**Files:**
- Modify: `src/cpu/decode.rs`
- Modify: `src/cpu/execute.rs`

- [ ] **Step 1: Add `SfenceVma` to the `Instruction` enum in `decode.rs`**

After `Wfi` in the `// --- System ---` section:
```rust
SfenceVma, // opcode 0x73, funct3=0, funct7=0x09 — flush TLB (Priv §10)
```

- [ ] **Step 2: Add decode arm for `SfenceVma`**

In the `funct3 = 0x0` match arm (under `0x73`), add before the `_` wildcard:

```rust
// SFENCE.VMA: funct7=0x09; csr field = (funct7<<5)|rs2 = 0x120..=0x13F
(csr, _, _) if (csr >> 5) == 0x09 => Ok(Instruction::SfenceVma),
```

The updated match block:
```rust
0x0 => match (csr, rs1, rd) {
    (0x000, 0, 0) => Ok(Instruction::Ecall),
    (0x001, 0, 0) => Ok(Instruction::Ebreak),
    (0x102, 0, 0) => Ok(Instruction::Sret),
    (0x302, 0, 0) => Ok(Instruction::Mret),
    (0x105, 0, 0) => Ok(Instruction::Wfi),
    (csr, _, _) if (csr >> 5) == 0x09 => Ok(Instruction::SfenceVma),
    _ => Err(anyhow::Error::new(IllegalInstruction(inst))),
},
```

- [ ] **Step 3: Add execute arm for `SfenceVma` in `execute.rs`**

After the `Instruction::Wfi` arm:
```rust
// SFENCE.VMA: full TLB flush. rs1/rs2 (address/ASID hints) ignored. Priv §10.
Instruction::SfenceVma => { cpu.mmu.flush(); },
```

- [ ] **Step 4: Run full test suite**

```bash
cargo test
```

Expected: 103+ tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/cpu/decode.rs src/cpu/execute.rs
git commit -m "feat: add SfenceVma instruction — full TLB flush"
```

---

### Task 10: Vendor rv64ui-v ELFs + run Phase 3 done criterion

**Files:**
- Modify: `scripts/fetch-riscv-tests.sh`
- Modify: `build.rs`
- Add: `tests/riscv-tests/rv64ui-v-*` ELFs

- [ ] **Step 1: Update `scripts/fetch-riscv-tests.sh`**

After the rv64si-p-* copy block, add:
```bash
echo "Copying rv64ui-v-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64ui-v-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;
```

And add to the summary:
```bash
echo "  rv64ui-v: $(ls "$DEST" | grep -c 'rv64ui-v-') ELFs"
```

- [ ] **Step 2: Run the fetch script**

```bash
bash scripts/fetch-riscv-tests.sh
```

Expected: copies rv64ui-v-* ELFs to `tests/riscv-tests/`. Typical count: ~40 ELFs (same tests as rv64ui-p-* but in virtual memory environment).

- [ ] **Step 3: Update `build.rs`**

Add to the `is_test_elf` predicate:
```rust
|| name.starts_with("rv64ui-v-")
```

Full updated predicate:
```rust
let is_test_elf = name.starts_with("rv64ui-p-")
    || name.starts_with("rv64um-p-")
    || name.starts_with("rv64ua-p-")
    || name.starts_with("rv64mi-p-")
    || name.starts_with("rv64si-p-")
    || name.starts_with("rv64ui-v-");
```

- [ ] **Step 4: Commit the ELFs + infra**

```bash
git add tests/riscv-tests/rv64ui-v-* scripts/fetch-riscv-tests.sh build.rs
git commit -m "test: vendor rv64ui-v-* ELFs and wire into test harness"
```

- [ ] **Step 5: Run rv64si gate (confirm all pass including rv64si-p-dirty)**

```bash
cargo test rv64si
```

Expected: all rv64si-p-* tests pass. If `rv64si-p-dirty` still fails at this point, trace it:
```bash
cargo run -- --trace tests/riscv-tests/rv64si-p-dirty 2>&1 | head -200
```

The test writes to a virtual page and checks that PTE.D is set afterward. Confirm the write goes through the MMU and hardware D-bit update fires.

- [ ] **Step 6: Run rv64ui-v gate**

```bash
cargo test rv64ui_v
```

Expected: all rv64ui-v-* tests pass. If any fail, trace:
```bash
cargo run -- --trace tests/riscv-tests/rv64ui-v-<failing> 2>&1 | head -200
```

Common failure modes:
- Page table walk not finding the leaf → check VPN extraction in translate()
- Permission denied → check U-bit / mode in check_perms()
- SATP not set → test preamble should set it; check SRET restores mode to U

- [ ] **Step 7: Run Phase 3 done criterion**

```bash
cargo test
```

Expected: all tests pass — at minimum:
- 54 rv64ui-p-*
- 13 rv64um-p-*
- 19 rv64ua-p-*
- 17 rv64mi-p-*
- 7 rv64si-p-*
- ~40 rv64ui-v-*

- [ ] **Step 8: Commit any fixes, then final commit**

```bash
git add src/cpu/
git commit -m "fix: <description>" # if needed

git commit --allow-empty -m "test: Phase 3 gate — all rv64si-p-* and rv64ui-v-* pass"
```
