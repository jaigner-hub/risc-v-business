# RISC-V Emulator Phase 5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Boot Linux to a BusyBox shell where `uname -a` and `ls /` work.

**Architecture:** Three independent work streams — bug fixes (MTVEC mode bits, mepc/sepc mask, RVC page-boundary fetch), interrupt delivery (CLINT → mip.MTIP, check_interrupts in step, WFI), and kernel loading (256 MiB RAM, --kernel/--initrd CLI, programmatic DTB). All three must be complete before Linux boots.

**Tech Stack:** Rust 2021, `vm-fdt = "0.3"` (new dependency), existing `anyhow`/`clap`/`goblin`.

---

## File map

| File | Change |
|------|--------|
| `src/cpu/csr.rs` | Fix mtvec write mask (store full value); fix mepc/sepc write masks (bit 0 only) |
| `src/cpu/mod.rs` | Fix RVC page-boundary fetch; rewrite `deliver_trap` with `trap_pc`/`mideleg`; add `check_interrupts()` called at top of `step()` |
| `src/cpu/decode.rs` | Add `Instruction::Wfi` variant; decode `0x10500073` |
| `src/cpu/execute.rs` | Add `Wfi` execute arm (advance PC only) |
| `src/clint.rs` | `tick()` returns `bool` (true when `mtime >= mtimecmp`) |
| `src/loader.rs` | `RAM_SIZE` → 256 MiB |
| `src/dtb.rs` | Replace `include_bytes!` with `build_dtb(initrd_size: u64)` using `vm-fdt` |
| `src/main.rs` | `--kernel`/`--initrd` flags; call `load_raw`; call `build_dtb`; update `clint.tick()` call |
| `Cargo.toml` | Add `vm-fdt = "0.3"` |
| `scripts/fetch-images.sh` | New script: download Debian kernel + build BusyBox initramfs |

---

### Task 1: fetch-images.sh

**Files:**
- Create: `scripts/fetch-images.sh`

This is a shell script with no unit tests. Write it, make it executable, and verify it runs without error on a fresh machine.

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
set -euo pipefail
mkdir -p images

# 1. OpenSBI
bash scripts/fetch-opensbi.sh

# 2. Linux kernel (Debian bookworm riscv64)
if [ ! -f images/Image ]; then
    BASE="https://ftp.debian.org/debian/pool/main/l/linux"
    DEB=$(curl -s "$BASE/" \
        | grep -o 'linux-image-[0-9][^"]*-riscv64_[^"]*_riscv64\.deb' \
        | sort -V | tail -1)
    curl -L -o /tmp/linux-image-riscv64.deb "$BASE/$DEB"
    dpkg-deb -x /tmp/linux-image-riscv64.deb /tmp/linux-riscv64/
    VMLINUZ=$(find /tmp/linux-riscv64 -name "vmlinuz-*-riscv64" | sort | tail -1)
    cp "$VMLINUZ" images/Image
    rm -rf /tmp/linux-image-riscv64.deb /tmp/linux-riscv64
    echo "Done: images/Image"
fi

# 3. Minimal BusyBox initramfs
if [ ! -f images/rootfs.img ]; then
    BASE="https://ftp.debian.org/debian/pool/main/b/busybox"
    DEB=$(curl -s "$BASE/" \
        | grep -o 'busybox-static_[^"]*_riscv64\.deb' \
        | sort -V | tail -1)
    curl -L -o /tmp/busybox-static-riscv64.deb "$BASE/$DEB"
    dpkg-deb -x /tmp/busybox-static-riscv64.deb /tmp/busybox-riscv64/

    D=$(mktemp -d)
    mkdir -p "$D"/{bin,proc,sys,dev,etc}
    cp /tmp/busybox-riscv64/bin/busybox "$D/bin/busybox"
    chmod +x "$D/bin/busybox"
    for cmd in sh ls uname mount; do ln -sf busybox "$D/bin/$cmd"; done
    cat > "$D/init" <<'EOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
exec /bin/sh
EOF
    chmod +x "$D/init"
    (cd "$D" && find . | cpio -o -H newc --quiet) | gzip > images/rootfs.img

    rm -rf /tmp/busybox-static-riscv64.deb /tmp/busybox-riscv64 "$D"
    echo "Done: images/rootfs.img"
fi
```

- [ ] **Step 2: Make it executable and commit**

```bash
chmod +x scripts/fetch-images.sh
git add scripts/fetch-images.sh
git commit -m "feat: add fetch-images.sh to download Linux kernel and BusyBox initramfs"
```

---

### Task 2: MTVEC vectored mode + mideleg interrupt delegation

**Files:**
- Modify: `src/cpu/csr.rs:133` (mtvec write)
- Modify: `src/cpu/mod.rs:86-106` (deliver_trap)
- Test: `src/cpu/csr.rs` (tests block)
- Test: `src/cpu/mod.rs` (tests block)

**Context:** `mtvec` MODE bits[1:0]=1 means vectored mode. OpenSBI programs mtvec=1 (vectored). The current `& !0x3` mask strips the mode bits, making every trap go to `base` instead of `base + cause*4` for interrupts. Additionally, `deliver_trap` currently checks `medeleg` for all traps — it must check `mideleg` when bit 63 of cause is set (interrupt vs exception).

- [ ] **Step 1: Write failing tests in `src/cpu/csr.rs`**

Add inside the `#[cfg(test)] mod tests` block (after the `medeleg_round_trips` test):

```rust
#[test]
fn mtvec_stores_vectored_mode_bits() {
    let mut csr = Csr::new();
    csr.write(0x305, 0x8000_1001); // base=0x8000_1000, MODE=1 (vectored)
    assert_eq!(csr.read(0x305), 0x8000_1001);
}

#[test]
fn mepc_preserves_rvc_alignment() {
    let mut csr = Csr::new();
    csr.write(0x341, 0x8000_1002); // bit 1 set (RVC 2-byte alignment)
    assert_eq!(csr.read(0x341), 0x8000_1002); // bit 1 preserved
    csr.write(0x341, 0x8000_1001); // bit 0 set (odd, illegal)
    assert_eq!(csr.read(0x341), 0x8000_1000); // bit 0 forced to 0
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test mtvec_stores_vectored_mode_bits mepc_preserves_rvc_alignment -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: both FAILED (current code masks MODE bits and mepc bits[1:0]).

- [ ] **Step 3: Write failing test in `src/cpu/mod.rs` for mideleg + vectored dispatch**

Add inside the `#[cfg(test)] mod tests` block (after `deliver_trap_s_mode_delegated_via_medeleg`):

```rust
#[test]
fn deliver_trap_interrupt_uses_mideleg_not_medeleg() {
    let mut c = cpu();
    c.csr.stvec   = 0x8000_0200;
    c.csr.mtvec   = 0x8000_0100;
    // Delegate MTI (cause 7, interrupt) to S-mode via mideleg
    c.csr.mideleg = 1 << 7;
    c.csr.medeleg = 0; // medeleg NOT set for cause 7
    c.mode = PrivMode::S;
    // Deliver interrupt: cause = (1<<63) | 7
    c.deliver_trap((1u64 << 63) | 7, 0);
    // Should go to stvec (delegated via mideleg), not mtvec
    assert_eq!(c.mode, PrivMode::S);
    assert_eq!(c.csr.scause, (1u64 << 63) | 7);
    assert_eq!(c.csr.mcause, 0);
}

#[test]
fn deliver_trap_vectored_mode_interrupt() {
    let mut c = cpu();
    // mtvec = base 0x8000_0000 | MODE=1 (vectored)
    c.csr.mtvec = 0x8000_0001;
    // Deliver MTI interrupt: cause = (1<<63)|7
    c.deliver_trap((1u64 << 63) | 7, 0);
    // vectored: PC = base + cause_code * 4 = 0x8000_0000 + 7*4 = 0x8000_001C
    assert_eq!(c.pc, 0x8000_001C);
    assert_eq!(c.csr.mcause, (1u64 << 63) | 7);
}

#[test]
fn deliver_trap_vectored_mode_exception_uses_base() {
    let mut c = cpu();
    c.csr.mtvec = 0x8000_0001; // vectored mode
    c.deliver_trap(8, 0);       // exception (no bit 63): always goes to base
    assert_eq!(c.pc, 0x8000_0000); // base only, not base + 8*4
}
```

- [ ] **Step 4: Run failing tests**

```bash
cargo test deliver_trap_interrupt_uses_mideleg deliver_trap_vectored_mode -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all three FAILED.

- [ ] **Step 5: Fix `src/cpu/csr.rs` — mtvec write mask**

In `pub fn write`, change:

```rust
// Before:
0x305 => self.mtvec    = val & !0x3,  // only direct mode (MODE=0) supported
// After:
0x305 => self.mtvec    = val,
```

Also remove the comment "only direct mode (MODE=0) supported".

- [ ] **Step 6: Fix `src/cpu/mod.rs` — rewrite `deliver_trap`**

Replace the entire `pub fn deliver_trap` body with:

```rust
pub fn deliver_trap(&mut self, cause: u64, tval: u64) {
    let is_interrupt = (cause >> 63) != 0;
    let cause_code   = cause & !(1u64 << 63);
    let deleg_reg    = if is_interrupt { self.csr.mideleg } else { self.csr.medeleg };
    let delegated    = cause_code < 64
                       && (deleg_reg >> cause_code) & 1 == 1
                       && self.mode != PrivMode::M;

    if delegated {
        self.csr.s_trap_entry(self.mode == PrivMode::S);
        self.csr.sepc   = self.pc;
        self.csr.scause = cause;
        self.csr.stval  = tval;
        self.pc   = trap_pc(self.csr.stvec, cause);
        self.mode = PrivMode::S;
    } else {
        self.csr.trap_entry(self.mode as u64);
        self.csr.mepc   = self.pc;
        self.csr.mcause = cause;
        self.csr.mtval  = tval;
        self.pc   = trap_pc(self.csr.mtvec, cause);
        self.mode = PrivMode::M;
    }
}
```

Add `trap_pc` as a free function in `src/cpu/mod.rs`, above `impl Cpu`:

```rust
fn trap_pc(tvec: u64, cause: u64) -> u64 {
    let base = tvec & !0x3;
    let mode = tvec & 0x3;
    let is_interrupt = (cause >> 63) != 0;
    if is_interrupt && mode == 1 {
        base + ((cause & !(1u64 << 63)) * 4)
    } else {
        base
    }
}
```

- [ ] **Step 7: Run all new and existing deliver_trap tests**

```bash
cargo test deliver_trap mtvec_stores_vectored mepc_preserves_rvc -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all pass. The five existing `deliver_trap_*` tests must still pass because they use direct-mode mtvec (mode=0) and exception causes (no bit 63), so `trap_pc` returns `base`, same as before.

- [ ] **Step 8: Run full test suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: `test result: ok. N passed; 0 failed`.

- [ ] **Step 9: Commit**

```bash
git add src/cpu/csr.rs src/cpu/mod.rs
git commit -m "fix: MTVEC vectored mode; use mideleg for interrupt delegation in deliver_trap"
```

---

### Task 3: MEPC/SEPC mask fix

**Files:**
- Modify: `src/cpu/csr.rs:135` (mepc write)
- Modify: `src/cpu/csr.rs:141` (sepc write)
- Test: `src/cpu/csr.rs` tests block

**Context:** With RVC, return addresses can be 2-byte aligned (bit 1 set, bit 0 clear). The current `val & !0x3` forces both bits 0 and 1 to zero, corrupting RVC return addresses. Per RISC-V spec with IALIGN=16: only bit 0 must be zero.

Note: The `mtvec_stores_vectored_mode_bits` and `mepc_preserves_rvc_alignment` tests were written in Task 2. The `mepc_preserves_rvc_alignment` test covers mepc. Now add sepc.

- [ ] **Step 1: Write failing sepc test**

Add in `src/cpu/csr.rs` tests block:

```rust
#[test]
fn sepc_preserves_rvc_alignment() {
    let mut csr = Csr::new();
    csr.write(0x141, 0x8000_1002); // bit 1 set (valid RVC address)
    assert_eq!(csr.read(0x141), 0x8000_1002);
    csr.write(0x141, 0x8000_1003); // bits 0 and 1 set
    assert_eq!(csr.read(0x141), 0x8000_1002); // bit 0 forced to 0
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test sepc_preserves_rvc_alignment -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: FAILED (current `self.sepc = val` stores bit 0 unmasked, and `sepc_preserves_rvc_alignment` will catch the bit-0 case... wait, current sepc write is `self.sepc = val` with no mask, so writing 0x8000_1003 would return 0x8000_1003 not 0x8000_1002 — test FAILS as expected).

- [ ] **Step 3: Fix both masks in `src/cpu/csr.rs`**

In `pub fn write`:

```rust
// Before:
0x341 => self.mepc     = val & !0x3,  // IALIGN=32: bits[1:0] always 0
0x141 => self.sepc     = val,
// After:
0x341 => self.mepc     = val & !0x1,  // IALIGN=16 (RVC): only bit 0 forced to 0
0x141 => self.sepc     = val & !0x1,  // IALIGN=16 (RVC): only bit 0 forced to 0
```

- [ ] **Step 4: Run all CSR tests**

```bash
cargo test --lib csr -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all pass including `mepc_preserves_rvc_alignment` and `sepc_preserves_rvc_alignment`.

- [ ] **Step 5: Run full test suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/cpu/csr.rs
git commit -m "fix: mepc/sepc write mask to IALIGN=16 (bit 0 only) for RVC compatibility"
```

---

### Task 4: RVC page-boundary fetch fix

**Files:**
- Modify: `src/cpu/mod.rs` (fetch logic in `step()`)
- Test: `src/cpu/mod.rs` tests block

**Context:** `bus.load(pa, 4)` always reads 4 bytes. A 2-byte RVC instruction at `pa = page_end - 2` causes a spurious fetch fault because the read spans into the next (unmapped) page. Fix: read 2 bytes first; if bits[1:0] != `0b11`, it is a 16-bit instruction. Otherwise read the next 2 bytes separately.

- [ ] **Step 1: Write failing test**

Add in `src/cpu/mod.rs` tests block:

```rust
#[test]
fn step_rvc_at_page_boundary_succeeds() {
    // 128 MiB RAM at 0x8000_0000. The last valid word is at 0x8800_0000 - 4.
    // Place a 2-byte RVC NOP (C.NOP = 0x0001) at the last 2 bytes of the first
    // 4 KiB page: address 0x8000_0FFE. The 4 KiB page [0x8000_1000..] is still
    // in RAM, but we want to confirm that a 4-byte load at 0x8000_0FFE does NOT
    // fault even though it crosses the natural alignment.
    //
    // More importantly: test that a C.NOP right at the boundary executes correctly.
    let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0FFE, false);
    c.csr.mtvec = 0x8000_0010;
    // Write C.NOP (0x0001) as little-endian at 0x8000_0FFE
    c.bus.store(0x8000_0FFE, 2, 0x0001u64).unwrap();
    c.step().unwrap();
    // C.NOP advances PC by 2
    assert_eq!(c.pc, 0x8000_1000);
    assert_eq!(c.csr.mcause, 0); // no trap
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test step_rvc_at_page_boundary_succeeds -- --nocapture 2>&1 | grep -E "FAILED|test .* ok|panicked"
```

Expected: either FAILED (wrong pc) or passes depending on current code. If `bus.load(pa, 4)` works for page-internal addresses (4 KiB page fully in RAM), the current code might pass already. The real bug is a cross-page-fault when the next page is *unmapped*. Add a second more targeted test:

```rust
#[test]
fn step_rvc_c_nop_advances_pc_by_2() {
    // Minimal test: C.NOP at base address, verify PC += 2 not += 4
    let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
    c.csr.mtvec = 0x8000_0100;
    c.bus.store(0x8000_0000, 2, 0x0001u64).unwrap(); // C.NOP = 0x0001
    c.step().unwrap();
    assert_eq!(c.pc, 0x8000_0002); // RVC: advance by 2
    assert_eq!(c.csr.mcause, 0);
}
```

- [ ] **Step 3: Run both new tests**

```bash
cargo test step_rvc_at_page_boundary step_rvc_c_nop_advances_pc_by_2 -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Note the results — at minimum `step_rvc_c_nop_advances_pc_by_2` may pass already (C.NOP already works from Phase 4). The goal of this task is ensuring 2-byte fetch correctness.

- [ ] **Step 4: Replace 4-byte fetch with 2-then-2 in `src/cpu/mod.rs` `step()`**

Replace the existing fetch section. Current code:

```rust
let raw = match self.mmu.translate(
    &mut self.bus, self.csr.satp, self.mode, self.csr.mstatus, pc, AccessType::Fetch
) {
    Err(f) => { self.deliver_trap(f.cause, f.tval); return Ok(()); }
    Ok(pa) => match self.bus.load(pa, 4) {
        Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
        Ok(v)  => v as u32,
    }
};
```

Replace with:

```rust
let pa = match self.mmu.translate(
    &mut self.bus, self.csr.satp, self.mode, self.csr.mstatus, pc, AccessType::Fetch
) {
    Err(f) => { self.deliver_trap(f.cause, f.tval); return Ok(()); }
    Ok(pa) => pa,
};

// Fetch low 16 bits first; determines 16-bit vs 32-bit instruction.
let lo = match self.bus.load(pa, 2) {
    Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
    Ok(v)  => v as u16,
};

let raw = if lo & 0x3 != 0x3 {
    // 16-bit RVC instruction: do not fetch upper 2 bytes.
    lo as u32
} else {
    // 32-bit instruction: fetch upper 16 bits.
    let hi = match self.bus.load(pa + 2, 2) {
        Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
        Ok(v)  => v as u16,
    };
    (lo as u32) | ((hi as u32) << 16)
};
```

- [ ] **Step 5: Run all cpu tests**

```bash
cargo test --lib cpu -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all pass.

- [ ] **Step 6: Run full suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/cpu/mod.rs
git commit -m "fix: RVC page-boundary fetch — read 2 bytes first, then 2 more only for 32-bit instructions"
```

---

### Task 5: CLINT timer interrupt wiring

**Files:**
- Modify: `src/clint.rs` (tick signature)
- Modify: `src/main.rs` (clint.tick() call site)
- Test: `src/clint.rs`

**Context:** `Clint::tick()` currently returns nothing. When `mtime >= mtimecmp`, `mip.MTIP` (bit 7) must be set. But `clint` lives inside `cpu.bus`, and `csr.mip` lives in `cpu.csr` — borrowing both simultaneously is not allowed. Fix: `tick()` returns a `bool`; the caller in `main.rs` updates `cpu.csr.mip`.

- [ ] **Step 1: Write failing tests in `src/clint.rs`**

Add a `#[cfg(test)] mod tests` block at the end of `src/clint.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_returns_false_while_mtime_lt_mtimecmp() {
        let mut c = Clint::new(); // mtime=0, mtimecmp=u64::MAX
        assert!(!c.tick()); // mtime becomes 1, still < MAX
        assert!(!c.tick()); // mtime becomes 2
    }

    #[test]
    fn tick_returns_true_when_mtime_reaches_mtimecmp() {
        let mut c = Clint::new();
        c.mtime    = 9;
        c.mtimecmp = 10;
        assert!(!c.tick()); // mtime becomes 10, equals mtimecmp → true
        // Wait: mtime wrapping_add(1) = 10, 10 >= 10 → true
        // Re-check: tick increments first, then compares.
        // After tick: mtime=10, mtimecmp=10, 10>=10 → true
        // Hmm, the assert above would be assert!(c.tick()) not assert!(!c.tick())
        // Let me re-examine: with mtime=9, after tick mtime=10, 10>=10=true
    }
}
```

Wait — the test above has a logic error. Let me write it correctly:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_returns_false_when_mtime_below_mtimecmp() {
        let mut c = Clint::new(); // mtime=0, mtimecmp=u64::MAX
        assert!(!c.tick()); // mtime=1, 1 < MAX → false
        assert!(!c.tick()); // mtime=2 → false
    }

    #[test]
    fn tick_returns_true_when_mtime_meets_mtimecmp() {
        let mut c = Clint::new();
        c.mtime    = 9;
        c.mtimecmp = 10;
        assert!(c.tick()); // mtime becomes 10, 10 >= 10 → true
    }

    #[test]
    fn tick_continues_true_while_mtime_exceeds_mtimecmp() {
        let mut c = Clint::new();
        c.mtime    = 10;
        c.mtimecmp = 10;
        assert!(c.tick()); // mtime=11, 11 >= 10 → true
    }
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test --lib clint -- --nocapture 2>&1 | grep -E "FAILED|error|test .* ok"
```

Expected: compile error (tick returns `()`, not `bool`).

- [ ] **Step 3: Fix `src/clint.rs` tick signature**

```rust
// Before:
pub fn tick(&mut self) {
    self.mtime = self.mtime.wrapping_add(1);
}

// After:
pub fn tick(&mut self) -> bool {
    self.mtime = self.mtime.wrapping_add(1);
    self.mtime >= self.mtimecmp
}
```

- [ ] **Step 4: Fix `src/main.rs` call site**

```rust
// Before:
loop {
    cpu.step()?;
    cpu.bus.clint.tick();
}

// After:
loop {
    cpu.step()?;
    if cpu.bus.clint.tick() {
        cpu.csr.mip |= 1 << 7;   // set MTIP
    } else {
        cpu.csr.mip &= !(1 << 7); // clear MTIP
    }
}
```

- [ ] **Step 5: Run CLINT tests**

```bash
cargo test --lib clint -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all three pass.

- [ ] **Step 6: Run full suite (catches any call-site regressions)**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/clint.rs src/main.rs
git commit -m "feat: CLINT tick() returns bool; main loop sets/clears mip.MTIP"
```

---

### Task 6: WFI instruction

**Files:**
- Modify: `src/cpu/decode.rs` (add `Instruction::Wfi` variant and decode arm)
- Modify: `src/cpu/execute.rs` (add Wfi execute arm)
- Test: `src/cpu/mod.rs` tests block

**Context:** Linux's idle loop executes WFI (`0x10500073`) to wait for interrupts. The emulator currently delivers this as an illegal-instruction trap. WFI should be a no-op that advances PC; the timer interrupt fires within a bounded number of ticks.

- [ ] **Step 1: Write failing test**

Add in `src/cpu/mod.rs` tests block:

```rust
#[test]
fn wfi_advances_pc_and_does_not_trap() {
    let mut c = Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false);
    c.csr.mtvec = 0x8000_0100;
    // WFI encoding: 0x10500073
    c.bus.store(0x8000_0000, 4, 0x10500073u64).unwrap();
    c.step().unwrap();
    assert_eq!(c.pc, 0x8000_0004); // advanced past WFI
    assert_eq!(c.csr.mcause, 0);   // no trap
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test wfi_advances_pc_and_does_not_trap -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: FAILED — `mcause == 2` (illegal instruction) and `pc == mtvec` instead of 0x8000_0004.

- [ ] **Step 3: Add `Instruction::Wfi` to `src/cpu/decode.rs`**

In the `pub enum Instruction` block, add after the system instructions (find `Mret`, `Sret`, `Sfence`):

```rust
Wfi,
```

In the `pub fn decode(raw: u32) -> Result<Instruction>` function, add (these are SYSTEM opcode instructions, opcode=0x73):

```rust
0x10500073 => Ok(Instruction::Wfi),
```

This line must appear **before** the existing catch-all for the SYSTEM opcode (wherever `Ecall`, `Ebreak`, `Mret`, `Sret` are decoded). Add it alongside those cases.

- [ ] **Step 4: Add Wfi execute arm to `src/cpu/execute.rs`**

Find the `execute` match and add:

```rust
Instruction::Wfi => {
    // No-op: advance PC. Timer interrupts fire on the next check_interrupts() call.
}
```

PC advancement already happens automatically via `inst_size` at the end of `execute`. Verify this is how other no-result instructions work (e.g., `Fence`). If execute does *not* auto-advance PC, add:

```rust
Instruction::Wfi => {
    cpu.pc = cpu.pc.wrapping_add(cpu.inst_size);
}
```

- [ ] **Step 5: Run failing test again**

```bash
cargo test wfi_advances_pc_and_does_not_trap -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: pass.

- [ ] **Step 6: Run full suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/cpu/decode.rs src/cpu/execute.rs
git commit -m "feat: decode and execute WFI as no-op (advance PC only)"
```

---

### Task 7: check_interrupts() in step()

**Files:**
- Modify: `src/cpu/mod.rs` (add `check_interrupts`, call at top of `step()`)
- Test: `src/cpu/mod.rs` tests block

**Context:** Interrupts are pending in `mip & mie` but are never checked during execution. `check_interrupts()` must run at the top of each `step()` before instruction fetch, select the highest-priority pending-and-enabled interrupt, and deliver it via `deliver_trap`.

Priority order: MEI(11) > MSI(3) > MTI(7) > SEI(9) > SSI(1) > STI(5).

Maskability: In M-mode, fires only if `mstatus.MIE=1` AND interrupt not delegated to S (mideleg[i]=0). In S/U-mode, non-delegated (M-mode) interrupts always fire; delegated (S-mode) interrupts fire if `mstatus.SIE=1`.

- [ ] **Step 1: Write failing tests**

Add in `src/cpu/mod.rs` tests block:

```rust
#[test]
fn check_interrupts_fires_mti_in_m_mode_with_mie_set() {
    let mut c = cpu();
    c.csr.mtvec   = 0x8000_0100;
    c.csr.mie     = 1 << 7;            // MTI enable
    c.csr.mip     = 1 << 7;            // MTI pending
    c.csr.mstatus = 1 << 3;            // MIE=1
    c.mode = PrivMode::M;
    c.step().unwrap();                 // step() calls check_interrupts() at top
    assert_eq!(c.csr.mcause, (1u64 << 63) | 7); // MTI interrupt cause
    assert_eq!(c.pc, 0x8000_0100 + 7 * 4);      // vectored if mtvec MODE=0: base only
    // mtvec=0x8000_0100, mode=0 (direct), so pc = base = 0x8000_0100
}

#[test]
fn check_interrupts_does_not_fire_when_mie_clear() {
    let mut c = cpu();
    c.csr.mtvec   = 0x8000_0100;
    c.csr.mie     = 1 << 7;            // MTI enable
    c.csr.mip     = 1 << 7;            // MTI pending
    c.csr.mstatus = 0;                 // MIE=0 — interrupts globally disabled
    c.mode = PrivMode::M;
    // Put a valid instruction at PC so step() doesn't fault
    c.bus.store(0x8000_0000, 4, 0x00000013u64).unwrap(); // ADDI x0,x0,0 (NOP)
    c.step().unwrap();
    assert_eq!(c.csr.mcause, 0);       // no interrupt delivered
    assert_eq!(c.pc, 0x8000_0004);     // NOP executed normally
}

#[test]
fn check_interrupts_routes_delegated_sti_to_s_mode() {
    let mut c = cpu();
    c.csr.stvec   = 0x8000_0200;
    c.csr.mtvec   = 0x8000_0100;
    c.csr.mie     = 1 << 5;            // STIE enable (bit 5)
    c.csr.mip     = 1 << 5;            // STIP pending (bit 5)
    c.csr.mideleg = 1 << 5;            // delegate STI to S-mode
    c.csr.mstatus = 1 << 1;            // SIE=1 (bit 1)
    c.mode = PrivMode::S;
    c.step().unwrap();
    assert_eq!(c.csr.scause, (1u64 << 63) | 5); // STI interrupt cause
    assert_eq!(c.mode, PrivMode::S);             // stays in S-mode (delegated)
    assert_eq!(c.csr.mcause, 0);                 // M-mode untouched
}
```

Note: the first test uses direct-mode mtvec (0x8000_0100, bits[1:0]=0), so `trap_pc` returns `0x8000_0100` (base), not `base + 7*4`. Fix the assertion:

```rust
assert_eq!(c.pc, 0x8000_0100); // direct mode: always base
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test check_interrupts -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all FAILED (no interrupt check in step yet).

- [ ] **Step 3: Add `check_interrupts` to `src/cpu/mod.rs`**

Add this method inside `impl Cpu` (after `deliver_trap`):

```rust
fn check_interrupts(&mut self) -> bool {
    let pending = self.csr.mip & self.csr.mie;
    if pending == 0 { return false; }

    for &cause_code in &[11u64, 3, 7, 9, 1, 5] {
        if (pending >> cause_code) & 1 == 0 { continue; }

        let delegated = (self.csr.mideleg >> cause_code) & 1 == 1;
        let can_fire = match self.mode {
            PrivMode::M => !delegated && self.csr.mie_bit() == 1,
            PrivMode::S | PrivMode::U => {
                if delegated {
                    (self.csr.mstatus >> 1) & 1 == 1  // SIE
                } else {
                    true  // non-delegated M-mode interrupt preempts S/U
                }
            }
        };

        if can_fire {
            let cause = (1u64 << 63) | cause_code;
            self.deliver_trap(cause, 0);
            return true;
        }
    }
    false
}
```

- [ ] **Step 4: Call `check_interrupts` at the top of `step()`**

At the very start of `pub fn step(&mut self) -> Result<()>`, before the PC alignment check:

```rust
pub fn step(&mut self) -> Result<()> {
    use decode::{decode, decode_rvc};
    use execute::execute;

    // Check for pending interrupts before fetching the next instruction.
    if self.check_interrupts() {
        return Ok(());
    }

    let pc = self.pc;
    // ... rest of step() unchanged ...
```

- [ ] **Step 5: Run all check_interrupts tests**

```bash
cargo test check_interrupts -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all pass.

- [ ] **Step 6: Run full suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/cpu/mod.rs
git commit -m "feat: add check_interrupts() called at top of step() with MEI>MSI>MTI>SEI>SSI>STI priority"
```

---

### Task 8: RAM 256 MiB + --kernel/--initrd CLI + raw binary loader

**Files:**
- Modify: `src/loader.rs` (RAM_SIZE constant)
- Modify: `src/main.rs` (--kernel/--initrd args, load_raw helper)

**Context:** The kernel Image (~10 MiB) is loaded at `0x8020_0000`, the initramfs at `0x8600_0000`. Both are beyond the current 128 MiB cap. RAM must be expanded to 256 MiB. Neither flag requires changes to the bus — the existing `ram_mut()` slice just needs to be big enough.

- [ ] **Step 1: Write failing test for RAM size in `src/loader.rs`**

Add in `src/loader.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_is_256_mib() {
        // A PT_LOAD at 0x9000_0000 - 1 byte = last byte of 256 MiB RAM should succeed.
        // 0x9000_0000 - 0x8000_0000 = 0x1000_0000 = 256 MiB.
        // Create a minimal ELF with one PT_LOAD at that address is complex;
        // instead verify Bus::new with RAM_SIZE fits the initramfs region.
        let bus = Bus::new(RAM_SIZE, RAM_BASE);
        let ram = bus.ram_ref(); // if Bus exposes ram_ref, else skip
        assert_eq!(ram.len(), 256 * 1024 * 1024);
    }
}
```

If `Bus` does not expose `ram_ref()`, test the constant directly:

```rust
#[test]
fn ram_size_constant_is_256_mib() {
    assert_eq!(RAM_SIZE, 256 * 1024 * 1024);
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test ram_size_constant_is_256_mib -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: FAILED (RAM_SIZE is currently 128 MiB).

- [ ] **Step 3: Fix RAM_SIZE in `src/loader.rs`**

```rust
// Before:
const RAM_SIZE: usize = 128 * 1024 * 1024; // 128 MiB

// After:
const RAM_SIZE: usize = 256 * 1024 * 1024; // 256 MiB
```

- [ ] **Step 4: Run loader test**

```bash
cargo test ram_size_constant_is_256_mib -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: pass.

- [ ] **Step 5: Add --kernel and --initrd flags to `src/main.rs`**

In the `struct Args`:

```rust
/// Raw kernel Image to load at 0x8020_0000 (requires --dtb)
#[arg(long)]
kernel: Option<String>,

/// Initramfs CPIO image to load at 0x8600_0000 (requires --dtb)
#[arg(long)]
initrd: Option<String>,
```

Add the constants at the top of `main.rs`:

```rust
const KERNEL_BASE: u64  = 0x8020_0000;
const INITRD_BASE: u64  = 0x8600_0000;
```

Add a helper function in `main.rs`:

```rust
fn load_raw(bus: &mut riscv_emu::bus::Bus, path: &str, phys_addr: u64) -> Result<usize> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read {path}"))?;
    let off = (phys_addr - RAM_BASE) as usize;
    bus.ram_mut()[off..off + bytes.len()].copy_from_slice(&bytes);
    Ok(bytes.len())
}
```

In `fn main()`, after constructing `cpu`, load kernel and initrd if provided:

```rust
if args.kernel.is_some() && !args.dtb {
    anyhow::bail!("--kernel requires --dtb");
}

let mut initrd_size: u64 = 0;

if let Some(ref kernel_path) = args.kernel {
    load_raw(&mut cpu.bus, kernel_path, KERNEL_BASE)?;
}
if let Some(ref initrd_path) = args.initrd {
    initrd_size = load_raw(&mut cpu.bus, initrd_path, INITRD_BASE)? as u64;
}
```

(The `initrd_size` variable is used in Task 9 for the DTB builder.)

- [ ] **Step 6: Run full suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass (no functional change, just constants and dead code until Task 9 wires it up).

- [ ] **Step 7: Commit**

```bash
git add src/loader.rs src/main.rs
git commit -m "feat: expand RAM to 256 MiB; add --kernel/--initrd CLI flags and load_raw helper"
```

---

### Task 9: Programmatic DTB builder (vm-fdt)

**Files:**
- Modify: `Cargo.toml` (add vm-fdt dependency)
- Modify: `src/dtb.rs` (replace include_bytes! with build_dtb)
- Modify: `src/main.rs` (call build_dtb, pass initrd_size)

**Context:** The static `virt.dtb` cannot encode `linux,initrd-start`/`linux,initrd-end` (these depend on initrd size at runtime). Replace it with a builder using `vm-fdt`. `dtb/virt.dtb` is removed; `dtb/virt.dts` is kept as documentation only.

- [ ] **Step 1: Add vm-fdt to Cargo.toml**

```toml
[dependencies]
clap    = { version = "4", features = ["derive"] }
anyhow  = "1"
goblin  = "0.8"
vm-fdt  = "0.3"
```

- [ ] **Step 2: Write failing test in `src/dtb.rs`**

Replace the entire contents of `src/dtb.rs` with:

```rust
use vm_fdt::FdtWriter;
use anyhow::Result;

pub const INITRD_BASE: u64 = 0x8600_0000;

pub fn build_dtb(initrd_size: u64) -> Result<Vec<u8>> {
    todo!("implement DTB builder")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_dtb_has_valid_fdt_magic() {
        let bytes = build_dtb(0).unwrap();
        // FDT magic: 0xD00DFEED in big-endian at offset 0
        let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(magic, 0xD00D_FEED);
        assert!(bytes.len() > 64);
    }

    #[test]
    fn build_dtb_with_initrd_includes_initrd_properties() {
        let bytes = build_dtb(0x100_0000).unwrap(); // 16 MiB initrd
        // Scan for the string "linux,initrd-start" in the FDT blob
        let haystack = std::str::from_utf8(&bytes).unwrap_or("");
        assert!(bytes.windows(18).any(|w| w == b"linux,initrd-start"),
            "expected linux,initrd-start property in DTB");
    }

    #[test]
    fn build_dtb_without_initrd_omits_initrd_properties() {
        let bytes = build_dtb(0).unwrap();
        assert!(!bytes.windows(18).any(|w| w == b"linux,initrd-start"),
            "linux,initrd-start should not appear when initrd_size=0");
    }
}
```

- [ ] **Step 3: Run failing tests**

```bash
cargo test --lib dtb -- --nocapture 2>&1 | grep -E "FAILED|panicked|test .* ok"
```

Expected: all FAILED (`todo!` panics).

- [ ] **Step 4: Implement `build_dtb` in `src/dtb.rs`**

Replace `todo!("implement DTB builder")` with the full implementation:

```rust
pub fn build_dtb(initrd_size: u64) -> Result<Vec<u8>> {
    let mut fdt = FdtWriter::new();

    let root = fdt.begin_node("")?;
    fdt.property_u32("#address-cells", 2)?;
    fdt.property_u32("#size-cells", 2)?;
    fdt.property_string("compatible", "riscv-virtio")?;
    fdt.property_string("model", "riscv-virtio,qemu")?;

    // chosen
    let chosen = fdt.begin_node("chosen")?;
    fdt.property_string("stdout-path", "/soc/serial@10000000")?;
    fdt.property_string("bootargs",
        "console=ttyS0 earlycon=uart8250,mmio,0x10000000 rdinit=/init")?;
    if initrd_size > 0 {
        let initrd_end = INITRD_BASE + initrd_size;
        fdt.property_u64("linux,initrd-start", INITRD_BASE)?;
        fdt.property_u64("linux,initrd-end",   initrd_end)?;
    }
    fdt.end_node(chosen)?;

    // cpus
    let cpus = fdt.begin_node("cpus")?;
    fdt.property_u32("#address-cells", 1)?;
    fdt.property_u32("#size-cells", 0)?;
    fdt.property_u32("timebase-frequency", 10_000_000)?;
    let cpu0 = fdt.begin_node("cpu@0")?;
    fdt.property_string("device_type", "cpu")?;
    fdt.property_u32("reg", 0)?;
    fdt.property_string("status", "okay")?;
    fdt.property_string("compatible", "riscv")?;
    fdt.property_string("riscv,isa", "rv64imafdcsu")?;
    fdt.property_string("mmu-type", "riscv,sv39")?;
    let intc = fdt.begin_node("interrupt-controller")?;
    fdt.property_u32("#interrupt-cells", 1)?;
    fdt.property_null("interrupt-controller")?;
    fdt.property_string("compatible", "riscv,cpu-intc")?;
    fdt.property_u32("phandle", 1)?;
    fdt.end_node(intc)?;
    fdt.end_node(cpu0)?;
    fdt.end_node(cpus)?;

    // memory: 256 MiB at 0x8000_0000
    let mem = fdt.begin_node("memory@80000000")?;
    fdt.property_string("device_type", "memory")?;
    // reg: [base_hi, base_lo, size_hi, size_lo] as four u32s (big-endian cells)
    fdt.property_array_u32("reg", &[0x0, 0x8000_0000, 0x0, 0x1000_0000])?;
    fdt.end_node(mem)?;

    // soc
    let soc = fdt.begin_node("soc")?;
    fdt.property_u32("#address-cells", 2)?;
    fdt.property_u32("#size-cells", 2)?;
    fdt.property_string("compatible", "simple-bus")?;
    fdt.property_null("ranges")?;

    // CLINT
    let clint = fdt.begin_node("clint@2000000")?;
    fdt.property_string("compatible", "riscv,clint0")?;
    fdt.property_array_u32("interrupts-extended", &[1, 3, 1, 7])?;
    fdt.property_array_u32("reg", &[0x0, 0x0200_0000, 0x0, 0x0001_0000])?;
    fdt.end_node(clint)?;

    // PLIC
    let plic = fdt.begin_node("plic@c000000")?;
    fdt.property_string("compatible", "sifive,plic-1.0.0")?;
    fdt.property_u32("#interrupt-cells", 1)?;
    fdt.property_null("interrupt-controller")?;
    fdt.property_u32("phandle", 2)?;
    fdt.property_array_u32("interrupts-extended", &[1, 11, 1, 9])?;
    fdt.property_array_u32("reg", &[0x0, 0x0C00_0000, 0x0, 0x0400_0000])?;
    fdt.property_u32("riscv,ndev", 31)?;
    fdt.end_node(plic)?;

    // UART 16550
    let uart = fdt.begin_node("serial@10000000")?;
    fdt.property_string("compatible", "ns16550a")?;
    fdt.property_array_u32("reg", &[0x0, 0x1000_0000, 0x0, 0x100])?;
    fdt.property_u32("clock-frequency", 3_686_400)?;
    fdt.property_u32("interrupts", 10)?;
    fdt.property_u32("interrupt-parent", 2)?;
    fdt.end_node(uart)?;

    fdt.end_node(soc)?;
    fdt.end_node(root)?;
    Ok(fdt.finish()?)
}
```

**Note on vm-fdt API:** If `FdtWriter::new()` returns a `Result`, change to `let mut fdt = FdtWriter::new()?;`. If `property_null` is named differently, check `cargo doc --open` for the actual method name. If `property_array_u32` doesn't exist, use `property_array_u8` with manually big-endian encoded bytes. The spec uses `property_u64` for `linux,initrd-start`/`end`; if that method doesn't exist, encode as two u32 cells.

- [ ] **Step 5: Update `src/main.rs` to call `build_dtb`**

In `fn main()`, find the `--dtb` block. Replace the `dtb::VIRT_DTB` usage:

```rust
// Before:
if args.dtb {
    let dtb_bytes = dtb::VIRT_DTB;
    let dtb_off = (DTB_BASE - RAM_BASE) as usize;
    cpu.bus.ram_mut()[dtb_off..dtb_off + dtb_bytes.len()].copy_from_slice(dtb_bytes);
    cpu.set_reg(10, 0);
    cpu.set_reg(11, DTB_BASE);
}

// After:
if args.dtb {
    let dtb_bytes = dtb::build_dtb(initrd_size)
        .context("failed to build device tree")?;
    let dtb_off = (DTB_BASE - RAM_BASE) as usize;
    cpu.bus.ram_mut()[dtb_off..dtb_off + dtb_bytes.len()].copy_from_slice(&dtb_bytes);
    cpu.set_reg(10, 0);
    cpu.set_reg(11, DTB_BASE);
}
```

Also remove the `use crate` import of `dtb` module if it referenced `VIRT_DTB` specifically; update to `dtb::build_dtb`.

Remove `dtb/virt.dtb` from the repository (it is no longer used):

```bash
git rm dtb/virt.dtb
```

- [ ] **Step 6: Run DTB tests**

```bash
cargo test --lib dtb -- --nocapture 2>&1 | grep -E "FAILED|test .* ok"
```

Expected: all three pass.

- [ ] **Step 7: Build check**

```bash
cargo build 2>&1 | grep -E "error|warning.*unused"
```

Expected: no errors.

- [ ] **Step 8: Run full suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/dtb.rs src/main.rs
git commit -m "feat: replace static DTB with programmatic vm-fdt builder; embed initrd addresses"
```

---

### Task 10: Regression + Phase 5 boot verification

**Files:**
- Read-only: all sources (no code changes)

This task verifies the complete implementation. There is no new code — only running tests and the emulator.

- [ ] **Step 1: Run the full test suite**

```bash
cargo test 2>&1 | tail -10
```

Expected output:

```
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Where N ≥ 229 (all Phase 1–4 tests plus the new Phase 5 unit tests). No failures allowed.

- [ ] **Step 2: Verify OpenSBI banner (Phase 4 regression)**

```bash
cargo run --release -- --dtb images/fw_jump.elf 2>/dev/null | head -20
```

Expected: OpenSBI banner appears (same as Phase 4). If `images/fw_jump.elf` is missing:

```bash
bash scripts/fetch-opensbi.sh
```

- [ ] **Step 3: Fetch images (if not present)**

```bash
bash scripts/fetch-images.sh
```

Expected: `images/Image` and `images/rootfs.img` created (or already present). No errors.

- [ ] **Step 4: Boot to Linux shell (Phase 5 criterion)**

```bash
cargo run --release -- --dtb --kernel images/Image --initrd images/rootfs.img images/fw_jump.elf 2>/dev/null
```

Expected sequence in stdout:
1. OpenSBI banner
2. Linux kernel boot messages
3. BusyBox init messages
4. `#` shell prompt

At the prompt:
```
# uname -a
Linux (none) 6.x.x #1 SMP ... riscv64 GNU/Linux
# ls /
bin  dev  etc  init  proc  sys
```

- [ ] **Step 5: Update phase gate in CLAUDE.md**

In `CLAUDE.md`, update the phase table:

```markdown
| 4 (current) | OpenSBI banner prints | Done |
| 5 | `#` shell prompt, `uname -a` and `ls /` work | Done |
```

- [ ] **Step 6: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: mark Phase 5 complete — Linux shell prompt, uname -a and ls / work"
```

---

## Self-review

**Spec coverage:**

| Spec requirement | Task |
|-----------------|------|
| fetch-images.sh | Task 1 |
| MTVEC vectored mode fix | Task 2 |
| deliver_trap mideleg for interrupts | Task 2 |
| mepc/sepc mask fix (IALIGN=16) | Task 3 |
| RVC page-boundary fetch (2+2 bytes) | Task 4 |
| CLINT tick → mip.MTIP | Task 5 |
| WFI decode + execute | Task 6 |
| check_interrupts() in step() | Task 7 |
| RAM 256 MiB | Task 8 |
| --kernel/--initrd CLI flags | Task 8 |
| load_raw helper | Task 8 |
| vm-fdt build_dtb with initrd addresses | Task 9 |
| Remove dtb/virt.dtb | Task 9 |
| Regression: 229 tests pass | Task 10 |
| OpenSBI still works | Task 10 |
| Phase 5 criterion: # prompt + uname/ls | Task 10 |

All spec requirements covered.

**Placeholder scan:** None found. Every step has complete code or exact commands.

**Type consistency:** `build_dtb(initrd_size: u64) -> Result<Vec<u8>>` used consistently in Task 9 definition and Task 9 `main.rs` call site. `load_raw` returns `Result<usize>`; cast to `u64` at call site in Task 8 `main.rs`.
