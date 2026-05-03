# RISC-V Emulator Phase 5 Design: Linux Shell Prompt

## Goal

Boot Linux to a BusyBox shell and verify `uname -a` and `ls /` work. Done criterion: `#` prompt appears on stdout after running `cargo run -- --dtb --kernel images/Image --initrd images/rootfs.img images/fw_jump.elf`.

## Architecture

Phase 5 has three independent areas of work that must all be complete before Linux boots:

1. **Bug fixes** — three correctness issues that prevent Linux from running
2. **Interrupt delivery** — timer interrupts are required for the Linux scheduler
3. **Kernel + initramfs loading** — load a raw kernel binary and initramfs into RAM, build the DTB dynamically with the right addresses

### Files modified

```
src/cpu/csr.rs        fix mtvec write mask; fix mepc write mask
src/cpu/mod.rs        interrupt check at top of step(); deliver_interrupt()
src/cpu/execute.rs    WFI instruction; vectored mtvec dispatch in deliver_trap
src/cpu/decode.rs     WFI decode
src/clint.rs          tick() sets/clears mip.MTIP based on mtime vs mtimecmp
src/dtb.rs            replace include_bytes! with programmatic DTB builder
src/main.rs           --kernel and --initrd CLI flags; raw binary loader; RAM 256 MiB
src/loader.rs         RAM_SIZE constant updated to 256 MiB
```

### Files added

```
scripts/fetch-images.sh   download Debian riscv64 kernel + build BusyBox initramfs
```

### Files removed

```
dtb/virt.dtb    no longer needed (DTB built at runtime)
```

`dtb/virt.dts` is kept as documentation but is no longer compiled or embedded.

---

## Memory map (256 MiB RAM)

| Region         | Address         | Notes                          |
|----------------|-----------------|--------------------------------|
| OpenSBI        | `0x8000_0000`   | fw_jump.elf entry, ~320 KiB    |
| Linux kernel   | `0x8020_0000`   | raw Image, loaded by emulator  |
| DTB            | `0x8220_0000`   | built at runtime, passed in a1 |
| Initramfs      | `0x8600_0000`   | rootfs.img, loaded by emulator |
| RAM end        | `0x9000_0000`   | 256 MiB total                  |

---

## Area 1: Bug fixes

### 1a. MTVEC vectored mode (`src/cpu/csr.rs`, `src/cpu/execute.rs`)

**Problem:** `mtvec` write masks bits[1:0] (`val & !0x3`), discarding the MODE field. OpenSBI programs mtvec with MODE=1 (vectored). Interrupt dispatch uses `base + cause_code * 4` in vectored mode; currently all traps go to `base`, which is wrong for interrupts.

**Fix — csr.rs write:**
```rust
0x305 => self.mtvec = val,  // store full value including MODE bits
```

**Fix — deliver_trap in mod.rs:**

Two changes to `deliver_trap`:

1. Use `mideleg` (not `medeleg`) when the cause is an interrupt (bit 63 set), so delegated interrupts are routed to S-mode correctly.
2. Use vectored dispatch for the target PC.

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

pub fn deliver_trap(&mut self, cause: u64, tval: u64) {
    let is_interrupt = (cause >> 63) != 0;
    let cause_code   = cause & !(1u64 << 63);
    let deleg_reg    = if is_interrupt { self.csr.mideleg } else { self.csr.medeleg };
    let delegated    = cause_code < 64 && (deleg_reg >> cause_code) & 1 == 1
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

### 1b. MEPC mask (`src/cpu/csr.rs`)

**Problem:** `mepc` write masks bits[1:0] (`val & !0x3`), corrupting RVC return addresses where bit 1 may be set.

**Fix:**
```rust
0x341 => self.mepc = val & !0x1,  // IALIGN=16 (RVC): only bit 0 forced to 0
```

Same fix for `sepc`:
```rust
0x141 => self.sepc = val & !0x1,
```

### 1c. RVC fetch across page boundary (`src/cpu/mod.rs`)

**Problem:** `bus.load(pa, 4)` always fetches 4 bytes. A 2-byte RVC instruction at the last 2 bytes of a mapped page (`pa & 0xFFF == 0xFFE`) causes a spurious instruction-access fault when the next page is unmapped.

**Fix:** Fetch 2 bytes first; if bits[1:0] != `0b11`, it's a 2-byte instruction — done. Otherwise fetch the upper 2 bytes separately.

```rust
// Fetch low 16 bits
let lo = match self.bus.load(pa, 2) {
    Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
    Ok(v)  => v as u16,
};

let raw = if lo & 0x3 != 0x3 {
    // 16-bit RVC instruction
    lo as u32
} else {
    // 32-bit instruction: fetch upper 16 bits
    let hi = match self.bus.load(pa + 2, 2) {
        Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
        Ok(v)  => v as u16,
    };
    (lo as u32) | ((hi as u32) << 16)
};
```

---

## Area 2: Interrupt delivery

### 2a. CLINT timer interrupt (`src/clint.rs`)

`Clint::tick()` increments `mtime` and returns whether `mtime >= mtimecmp`. The caller (main loop) updates `cpu.csr.mip` — this avoids a borrow conflict between `cpu.bus` and `cpu.csr`:

```rust
pub fn tick(&mut self) -> bool {
    self.mtime = self.mtime.wrapping_add(1);
    self.mtime >= self.mtimecmp
}
```

`main.rs` call site becomes:
```rust
if cpu.bus.clint.tick() {
    cpu.csr.mip |= 1 << 7;   // set MTIP
} else {
    cpu.csr.mip &= !(1 << 7); // clear MTIP
}

### 2b. Interrupt check in `step()` (`src/cpu/mod.rs`)

At the **top** of `step()`, before instruction fetch, check for pending interrupts:

```rust
// Interrupt check: any unmasked pending interrupt?
if self.check_interrupts() {
    return Ok(());
}
```

`check_interrupts()` determines whether an interrupt can be taken given current mode and global-enable bits, selects the highest-priority pending interrupt, and calls `deliver_interrupt(cause)`.

**Priority (highest first):** MEI(11) > MSI(3) > MTI(7) > SEI(9) > SSI(1) > STI(5)

**Maskability rules (Priv §3.1.9):**
- In M-mode: interrupt fires only if `mstatus.MIE=1` and `mip[i] & mie[i]` (unless delegated to S via mideleg, in which case it always fires)
- In S/U-mode: M-mode interrupts (not delegated) always fire; S-mode interrupts (mideleg[i]=1) fire if `mstatus.SIE=1`

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
                    true  // M-mode interrupt always preempts S/U
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

### 2c. WFI instruction (`src/cpu/decode.rs`, `src/cpu/execute.rs`)

WFI (`0x10500073`) is decoded as a new `Instruction::Wfi` variant and executed as a no-op (advance PC only). This is legal per the spec; Linux will re-enter WFI on the next loop iteration. Timer ticks continue accumulating, so the interrupt will fire within a bounded number of steps.

```rust
// decode.rs: in decode()
0x10500073 => Ok(Instruction::Wfi),

// execute.rs: in execute()
Instruction::Wfi => { /* advance PC, no other effect */ }
```

---

## Area 3: Kernel + initramfs loading

### 3a. CLI flags (`src/main.rs`)

```rust
/// Raw kernel Image to load at 0x8020_0000
#[arg(long)]
kernel: Option<String>,

/// Initramfs CPIO image to load at 0x8600_0000
#[arg(long)]
initrd: Option<String>,
```

Both are optional. When `--kernel` is supplied without `--dtb`, emit an error. Both are no-ops without `--dtb`.

### 3b. Raw binary loader

A new helper in `src/main.rs` (or `src/loader.rs`):

```rust
fn load_raw(bus: &mut Bus, path: &str, phys_addr: u64, ram_base: u64) -> Result<usize> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read {path}"))?;
    let off = (phys_addr - ram_base) as usize;
    bus.ram_mut()[off..off + bytes.len()].copy_from_slice(&bytes);
    Ok(bytes.len())
}
```

Loads kernel at `0x8020_0000`, initramfs at `0x8600_0000`.

### 3c. RAM size (`src/loader.rs`, `src/main.rs`)

```rust
// src/loader.rs
const RAM_SIZE: usize = 256 * 1024 * 1024; // 256 MiB
```

The DTB memory node is updated to match (see 3d).

### 3d. Programmatic DTB builder (`src/dtb.rs`)

Replace the `include_bytes!` blob with a builder using the `vm-fdt` crate.

```toml
# Cargo.toml
[dependencies]
vm-fdt = "0.3"
```

```rust
// src/dtb.rs
use vm_fdt::{FdtWriter, FdtWriterResult};

pub const INITRD_BASE: u64 = 0x8600_0000;

pub fn build_dtb(initrd_size: u64) -> FdtWriterResult<Vec<u8>> {
    let mut fdt = FdtWriter::new()?;

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
        let initrd_start = INITRD_BASE;
        let initrd_end   = INITRD_BASE + initrd_size;
        fdt.property_array_u64("linux,initrd-start", &[initrd_start])?;
        fdt.property_array_u64("linux,initrd-end",   &[initrd_end])?;
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
    fdt.property_u32("phandle", 1)?;   // phandle 1 = cpu0_intc
    fdt.end_node(intc)?;
    fdt.end_node(cpu0)?;
    fdt.end_node(cpus)?;

    // memory: 256 MiB at 0x8000_0000
    let mem = fdt.begin_node("memory@80000000")?;
    fdt.property_string("device_type", "memory")?;
    fdt.property_array_u64("reg", &[0x8000_0000, 0x1000_0000])?; // 256 MiB
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
    // interrupts-extended: cpu0_intc IRQ 3 (MSI), cpu0_intc IRQ 7 (MTI)
    fdt.property_array_u32("interrupts-extended", &[1, 3, 1, 7])?;
    fdt.property_array_u64("reg", &[0x0200_0000, 0x0001_0000])?;
    fdt.end_node(clint)?;

    // PLIC
    let plic = fdt.begin_node("plic@c000000")?;
    fdt.property_string("compatible", "sifive,plic-1.0.0")?;
    fdt.property_u32("#interrupt-cells", 1)?;
    fdt.property_null("interrupt-controller")?;
    fdt.property_u32("phandle", 2)?;
    // interrupts-extended: cpu0_intc IRQ 11 (MEI), cpu0_intc IRQ 9 (SEI)
    fdt.property_array_u32("interrupts-extended", &[1, 11, 1, 9])?;
    fdt.property_array_u64("reg", &[0x0C00_0000, 0x0400_0000])?;
    fdt.property_u32("riscv,ndev", 31)?;
    fdt.end_node(plic)?;

    // UART 16550
    let uart = fdt.begin_node("serial@10000000")?;
    fdt.property_string("compatible", "ns16550a")?;
    fdt.property_array_u64("reg", &[0x1000_0000, 0x100])?;
    fdt.property_u32("clock-frequency", 3_686_400)?;
    fdt.property_u32("interrupts", 10)?;
    fdt.property_u32("interrupt-parent", 2)?;   // phandle of PLIC
    fdt.end_node(uart)?;

    fdt.end_node(soc)?;
    fdt.end_node(root)?;
    fdt.finish()
}
```

`main.rs` calls `dtb::build_dtb(initrd_size)` and writes the result into RAM at `DTB_BASE`.

### 3e. fetch-images.sh (`scripts/fetch-images.sh`)

```bash
#!/usr/bin/env bash
set -euo pipefail
mkdir -p images

# 1. OpenSBI
bash scripts/fetch-opensbi.sh

# 2. Linux kernel (Debian bookworm riscv64)
if [ ! -f images/Image ]; then
    # Find the .deb URL from the Debian pool index
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

---

## Testing

### Regression: all 229 tests still pass
```bash
cargo test
```

### OpenSBI banner still works
```bash
cargo run --release -- --dtb images/fw_jump.elf
```

### Phase 5 criterion
```bash
bash scripts/fetch-images.sh
cargo run --release -- --dtb --kernel images/Image --initrd images/rootfs.img images/fw_jump.elf
```

Expected output: OpenSBI banner → Linux boot log → `#` prompt. Then verify:
```
# uname -a
Linux (none) 6.x.x #1 SMP ... riscv64 GNU/Linux
# ls /
bin  dev  etc  init  proc  sys
```
