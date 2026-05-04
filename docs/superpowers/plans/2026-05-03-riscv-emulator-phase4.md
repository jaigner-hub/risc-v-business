# RISC-V Emulator Phase 4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add CLINT, UART 16550, PLIC, and DTB support so that `cargo run -- images/fw_jump.elf --dtb` prints the OpenSBI banner to stdout.

**Architecture:** Bus gains three named device fields (clint, uart, plic); `load`/`store` dispatch by address range before falling through to RAM. A minimal DTB is embedded in the binary via `include_bytes!`; a `--dtb` boolean flag in main triggers OpenSBI boot mode (copies DTB into RAM at `0x8220_0000`, sets a0/a1, ticks CLINT each step).

**Tech Stack:** Rust, `std::io::Write` for UART flush, `device-tree-compiler` (dtc) for DTB compilation, `curl`/`tar` for OpenSBI download.

---

### Task 1: CLINT device

**Files:**
- Create: `src/clint.rs`

- [ ] **Step 1: Create `src/clint.rs`**

```rust
pub struct Clint {
    pub mtime: u64,
    pub mtimecmp: u64,
}

impl Clint {
    pub fn new() -> Self {
        Self { mtime: 0, mtimecmp: u64::MAX }
    }

    pub fn tick(&mut self) {
        self.mtime = self.mtime.wrapping_add(1);
    }

    pub fn load(&self, addr: u64, _width: usize) -> u64 {
        match addr {
            0x0200_BFF8 => self.mtime,
            0x0200_4000 => self.mtimecmp,
            _ => 0,
        }
    }

    pub fn store(&mut self, addr: u64, _width: usize, val: u64) {
        if addr == 0x0200_4000 {
            self.mtimecmp = val;
        }
    }
}
```

- [ ] **Step 2: Verify it compiles (don't wire it in yet)**

The file won't be referenced until Task 4, but we can check for syntax errors by temporarily adding `mod clint;` to `src/lib.rs`, building, then removing the line. Skip this if you're confident in the code.

- [ ] **Step 3: Commit**

```bash
git add src/clint.rs
git commit -m "feat(phase4): add CLINT device (mtime + mtimecmp)"
```

---

### Task 2: UART 16550 stub

**Files:**
- Create: `src/uart.rs`

- [ ] **Step 1: Create `src/uart.rs`**

```rust
use std::io::{self, Write};

pub struct Uart16550;

impl Uart16550 {
    pub fn new() -> Self {
        Self
    }

    pub fn load(&self, addr: u64, _width: usize) -> u64 {
        match addr & 0xFF {
            5 => 0x60, // LSR: TX-empty + THR-empty (bits 5+6)
            _ => 0,
        }
    }

    pub fn store(&mut self, addr: u64, _width: usize, val: u64) {
        if addr & 0xFF == 0 {
            // THR write: emit the character
            print!("{}", val as u8 as char);
            let _ = io::stdout().flush();
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add src/uart.rs
git commit -m "feat(phase4): add UART 16550 stub (LSR=0x60, THR→stdout)"
```

---

### Task 3: PLIC stub

**Files:**
- Create: `src/plic.rs`

- [ ] **Step 1: Create `src/plic.rs`**

```rust
pub struct Plic;

impl Plic {
    pub fn new() -> Self {
        Self
    }

    pub fn load(&self, _addr: u64, _width: usize) -> u64 {
        0
    }

    pub fn store(&mut self, _addr: u64, _width: usize, _val: u64) {}
}
```

- [ ] **Step 2: Commit**

```bash
git add src/plic.rs
git commit -m "feat(phase4): add PLIC stub (reads 0, drops writes)"
```

---

### Task 4: Bus refactor — add device fields and address dispatch

**Files:**
- Modify: `src/bus.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add the three new modules to `src/lib.rs`**

Replace:
```rust
pub mod bus;
pub mod cpu;
pub mod loader;
```

With:
```rust
pub mod bus;
pub mod clint;
pub mod cpu;
pub mod dtb;
pub mod loader;
pub mod plic;
pub mod uart;
```

> Note: `dtb` is declared here now so Task 5 doesn't need to touch `lib.rs` again.

- [ ] **Step 2: Rewrite `src/bus.rs`**

Replace the entire file with:

```rust
use anyhow::{anyhow, Result};
use crate::clint::Clint;
use crate::plic::Plic;
use crate::uart::Uart16550;

pub struct Bus {
    ram: Vec<u8>,
    pub ram_base: u64,
    pub clint: Clint,
    pub uart: Uart16550,
    pub plic: Plic,
}

impl Bus {
    pub fn new(size: usize, ram_base: u64) -> Self {
        Self {
            ram: vec![0u8; size],
            ram_base,
            clint: Clint::new(),
            uart: Uart16550::new(),
            plic: Plic::new(),
        }
    }

    pub fn ram_mut(&mut self) -> &mut Vec<u8> {
        &mut self.ram
    }

    /// Load `width` bytes (1/2/4/8) from `addr`, zero-extended to u64.
    pub fn load(&self, addr: u64, width: usize) -> Result<u64> {
        match addr {
            0x0200_0000..=0x0200_FFFF => Ok(self.clint.load(addr, width)),
            0x0C00_0000..=0x0FFF_FFFF => Ok(self.plic.load(addr, width)),
            0x1000_0000..=0x1000_00FF => Ok(self.uart.load(addr, width)),
            _ => {
                let off = self.offset(addr, width)?;
                Ok(match width {
                    1 => self.ram[off] as u64,
                    2 => u16::from_le_bytes(self.ram[off..off + 2].try_into().unwrap()) as u64,
                    4 => u32::from_le_bytes(self.ram[off..off + 4].try_into().unwrap()) as u64,
                    8 => u64::from_le_bytes(self.ram[off..off + 8].try_into().unwrap()),
                    _ => unreachable!("invalid load width {width}"),
                })
            }
        }
    }

    /// Store `width` bytes (1/2/4/8) of `value` to `addr`.
    pub fn store(&mut self, addr: u64, width: usize, value: u64) -> Result<()> {
        match addr {
            0x0200_0000..=0x0200_FFFF => { self.clint.store(addr, width, value); Ok(()) }
            0x0C00_0000..=0x0FFF_FFFF => { self.plic.store(addr, width, value); Ok(()) }
            0x1000_0000..=0x1000_00FF => { self.uart.store(addr, width, value); Ok(()) }
            _ => {
                let off = self.offset(addr, width)?;
                match width {
                    1 => self.ram[off] = value as u8,
                    2 => self.ram[off..off + 2].copy_from_slice(&(value as u16).to_le_bytes()),
                    4 => self.ram[off..off + 4].copy_from_slice(&(value as u32).to_le_bytes()),
                    8 => self.ram[off..off + 8].copy_from_slice(&value.to_le_bytes()),
                    _ => unreachable!("invalid store width {width}"),
                }
                Ok(())
            }
        }
    }

    fn offset(&self, addr: u64, width: usize) -> Result<usize> {
        let end = addr
            .checked_add(width as u64)
            .ok_or_else(|| anyhow!("bus fault: addr={addr:#x} width={width}"))?;
        let ram_end = self.ram_base + self.ram.len() as u64;
        if addr < self.ram_base || end > ram_end {
            return Err(anyhow!("bus fault: addr={addr:#x} width={width}"));
        }
        Ok((addr - self.ram_base) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> Bus {
        Bus::new(64, 0x8000_0000)
    }

    #[test]
    fn load_store_u8() {
        let mut b = bus();
        b.store(0x8000_0000, 1, 0xAB).unwrap();
        assert_eq!(b.load(0x8000_0000, 1).unwrap(), 0xAB);
    }

    #[test]
    fn load_store_u16_le() {
        let mut b = bus();
        b.store(0x8000_0000, 2, 0x1234).unwrap();
        assert_eq!(b.load(0x8000_0000, 1).unwrap(), 0x34);
        assert_eq!(b.load(0x8000_0001, 1).unwrap(), 0x12);
    }

    #[test]
    fn load_store_u64() {
        let mut b = bus();
        b.store(0x8000_0000, 8, 0xDEADBEEF_CAFEBABE).unwrap();
        assert_eq!(b.load(0x8000_0000, 8).unwrap(), 0xDEADBEEF_CAFEBABE);
    }

    #[test]
    fn out_of_bounds_returns_err() {
        let b = bus();
        assert!(b.load(0x0000_0000, 4).is_err());
        assert!(b.load(0x8000_0040, 4).is_err());
    }

    #[test]
    fn clint_mtime_readable() {
        let b = bus();
        assert_eq!(b.load(0x0200_BFF8, 8).unwrap(), 0);
    }

    #[test]
    fn uart_lsr_returns_0x60() {
        let b = bus();
        assert_eq!(b.load(0x1000_0005, 1).unwrap(), 0x60);
    }

    #[test]
    fn plic_reads_zero() {
        let b = bus();
        assert_eq!(b.load(0x0C00_0000, 4).unwrap(), 0);
    }
}
```

- [ ] **Step 3: Build and run all tests**

```bash
cargo build 2>&1 | tail -5
cargo test 2>&1 | tail -20
```

Expected: `test result: ok. 171 passed` (164 riscv-tests + 7 bus unit tests). All must pass.

- [ ] **Step 4: Commit**

```bash
git add src/bus.rs src/lib.rs
git commit -m "feat(phase4): bus dispatches to CLINT/UART/PLIC by address range"
```

---

### Task 5: DTB — compile and embed

**Files:**
- Create: `dtb/virt.dts`
- Create: `dtb/virt.dtb` (compiled, committed)
- Create: `src/dtb.rs`

> **One-time setup:** Install the device-tree compiler if not present:
> `sudo apt-get install -y device-tree-compiler`

- [ ] **Step 1: Create the `dtb/` directory and write `dtb/virt.dts`**

```bash
mkdir -p dtb
```

Write `dtb/virt.dts` with this exact content:

```dts
/dts-v1/;

/ {
    #address-cells = <2>;
    #size-cells = <2>;
    compatible = "riscv-virtio";
    model = "riscv-virtio,qemu";

    chosen {
        stdout-path = "/soc/serial@10000000";
    };

    cpus {
        #address-cells = <1>;
        #size-cells = <0>;
        timebase-frequency = <10000000>;

        cpu0: cpu@0 {
            device_type = "cpu";
            reg = <0>;
            status = "okay";
            compatible = "riscv";
            riscv,isa = "rv64imafdcsu";
            mmu-type = "riscv,sv39";

            cpu0_intc: interrupt-controller {
                #interrupt-cells = <1>;
                interrupt-controller;
                compatible = "riscv,cpu-intc";
            };
        };
    };

    memory@80000000 {
        device_type = "memory";
        reg = <0x0 0x80000000 0x0 0x8000000>;
    };

    soc {
        #address-cells = <2>;
        #size-cells = <2>;
        compatible = "simple-bus";
        ranges;

        clint@2000000 {
            compatible = "riscv,clint0";
            interrupts-extended = <&cpu0_intc 3 &cpu0_intc 7>;
            reg = <0x0 0x2000000 0x0 0x10000>;
        };

        plic: plic@c000000 {
            compatible = "sifive,plic-1.0.0", "riscv,plic0";
            #interrupt-cells = <1>;
            interrupt-controller;
            interrupts-extended = <&cpu0_intc 9 &cpu0_intc 11>;
            reg = <0x0 0xc000000 0x0 0x4000000>;
            riscv,ndev = <31>;
        };

        serial@10000000 {
            compatible = "ns16550a";
            reg = <0x0 0x10000000 0x0 0x100>;
            clock-frequency = <3686400>;
            interrupts = <10>;
            interrupt-parent = <&plic>;
        };
    };
};
```

- [ ] **Step 2: Compile the DTB**

```bash
dtc -I dts -O dtb -o dtb/virt.dtb dtb/virt.dts
```

Expected: no output, exit code 0. Verify: `ls -lh dtb/virt.dtb` — should be ~1–2 KiB.

- [ ] **Step 3: Create `src/dtb.rs`**

```rust
pub const VIRT_DTB: &[u8] = include_bytes!("../dtb/virt.dtb");
```

- [ ] **Step 4: Verify it compiles**

`lib.rs` already declares `pub mod dtb;` (added in Task 4). Run:

```bash
cargo build 2>&1 | tail -5
```

Expected: compiles cleanly.

- [ ] **Step 5: Commit the DTB files**

```bash
git add dtb/virt.dts dtb/virt.dtb src/dtb.rs
git commit -m "feat(phase4): add minimal virt DTB (compiled + embedded)"
```

---

### Task 6: `main.rs` — `--dtb` flag, DTB copy, a0/a1, CLINT tick

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Rewrite `src/main.rs`**

Replace the entire file with:

```rust
use anyhow::{Context, Result};
use clap::Parser;
use riscv_emu::{cpu::Cpu, dtb, loader};

#[derive(Parser)]
#[command(name = "riscv-emu", about = "RV64I emulator")]
struct Args {
    /// ELF binary to run
    binary: String,

    /// Enable per-instruction trace output on stderr
    #[arg(long)]
    trace: bool,

    /// Load embedded VIRT_DTB and boot in OpenSBI mode (sets a0/a1, ticks CLINT)
    #[arg(long)]
    dtb: bool,
}

const DTB_BASE: u64 = 0x8220_0000;
const RAM_BASE: u64 = 0x8000_0000;

fn main() -> Result<()> {
    let args = Args::parse();
    let bytes = std::fs::read(&args.binary)
        .with_context(|| format!("failed to read {}", args.binary))?;

    let loaded = loader::load_elf(&bytes)?;
    let mut cpu = Cpu::new(loaded.bus, loaded.entry, args.trace);

    if args.dtb {
        let dtb_bytes = dtb::VIRT_DTB;
        let dtb_off = (DTB_BASE - RAM_BASE) as usize;
        cpu.bus.ram_mut()[dtb_off..dtb_off + dtb_bytes.len()].copy_from_slice(dtb_bytes);
        cpu.set_reg(10, 0);              // a0 = hart ID 0
        cpu.set_reg(11, DTB_BASE);       // a1 = FDT pointer
    }

    loop {
        cpu.step()?;
        cpu.bus.clint.tick();
    }
}
```

- [ ] **Step 2: Build to confirm it compiles**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Finished dev` with no errors.

- [ ] **Step 3: Run all existing tests to confirm no regressions**

```bash
cargo test 2>&1 | tail -20
```

Expected: `test result: ok. 171 passed` (164 riscv-tests + 7 bus unit tests).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(phase4): add --dtb flag; copy DTB into RAM, set a0/a1, tick CLINT"
```

---

### Task 7: `scripts/fetch-opensbi.sh`

**Files:**
- Create: `scripts/fetch-opensbi.sh`

> The `images/` directory is already in `.gitignore` — do not commit the ELF binary.

- [ ] **Step 1: Create `scripts/fetch-opensbi.sh`**

```bash
#!/usr/bin/env bash
set -euo pipefail

VERSION="1.8.1"
TARBALL="opensbi-${VERSION}-rv-bin.tar.xz"
URL="https://github.com/riscv-software-src/opensbi/releases/download/v${VERSION}/${TARBALL}"
ELF_PATH="opensbi-${VERSION}-rv-bin/share/opensbi/lp64/generic/firmware/fw_jump.elf"

mkdir -p images

if [ -f "images/fw_jump.elf" ]; then
    echo "images/fw_jump.elf already exists — skipping download"
    exit 0
fi

echo "Downloading OpenSBI v${VERSION}..."
curl -L -o "/tmp/${TARBALL}" "${URL}"

echo "Extracting fw_jump.elf..."
tar -xf "/tmp/${TARBALL}" -C /tmp "${ELF_PATH}"
cp "/tmp/${ELF_PATH}" images/fw_jump.elf
rm -f "/tmp/${TARBALL}"
rm -rf "/tmp/opensbi-${VERSION}-rv-bin"

echo "Done: images/fw_jump.elf"
```

- [ ] **Step 2: Make executable and commit**

```bash
chmod +x scripts/fetch-opensbi.sh
git add scripts/fetch-opensbi.sh
git commit -m "feat(phase4): add fetch-opensbi.sh script"
```

---

### Task 8: Regression check and OpenSBI banner verification

**Files:** None (verification only)

- [ ] **Step 1: Run all 164 riscv-tests**

```bash
cargo test 2>&1 | tail -20
```

Expected:
```
test result: ok. 171 passed; 0 failed; 0 ignored
```

All 164 riscv-tests plus 7 bus unit tests must pass.

- [ ] **Step 2: Download OpenSBI**

```bash
bash scripts/fetch-opensbi.sh
```

Expected: `Done: images/fw_jump.elf`

- [ ] **Step 3: Run OpenSBI and observe the banner**

```bash
cargo run --release -- images/fw_jump.elf --dtb
```

Expected output (then a crash/fault — that is acceptable for Phase 4):

```
OpenSBI v1.x
   ____                    _____ ____ _____
  / __ \                  / ____|  _ \_   _|
 | |  | |_ __   ___ _ __ | (___ | |_) || |
 | |  | | '_ \ / _ \ '_ \ \___ \|  _ < | |
 | |__| | |_) |  __/ | | |____) | |_) || |_
  \____/| .__/ \___|_| |_|_____/|____/_____|
        | |
        |_|

Platform Name             : riscv-virtio,qemu
...
```

The emulator crashing after the banner (bus fault at `0x8020_0000`) is expected and completes Phase 4.

- [ ] **Step 4: Phase 4 gate cleared — commit any final cleanup**

If nothing else to change:

```bash
git status
# Should be clean — nothing to commit
```

Phase 4 is done. Proceed to Phase 5 (boot Linux to a shell prompt).
