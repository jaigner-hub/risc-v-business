# risc-v-business

A RISC-V emulator in Rust, built to boot mainline Linux.

**Target:** RV64GC (IMAFD + C), M/S/U privilege levels, Sv39 MMU, OpenSBI + Linux + BusyBox initramfs.

**Status:** Phase 5 complete — boots Debian's Linux 6.12 kernel to a BusyBox `#` shell prompt. `uname -a` and `ls /` work. 252 tests passing.

```
[    0.000000] Linux version 6.12.85+deb13-riscv64 ...
...
BusyBox v1.37.0 (Debian 1:1.37.0-10.1) built-in shell (ash)
~ # uname -a
Linux (none) 6.12.85+deb13-riscv64 #1 SMP Debian 6.12.85-1 (2026-04-30) riscv64 GNU/Linux
~ # ls /
bin   dev   etc   init  proc  root  sys
```

## Phases

| # | Scope | Status |
|---|-------|--------|
| 1 | RV64I interpreter, rv64ui-p-* (54 tests) | Done |
| 2 | M/A extensions, Zicsr, M-mode privilege/traps, rv64um/ua/mi-p-* | Done |
| 3 | S-mode privilege, Sv39 MMU, rv64si-p-* + rv64ui-v-* (164 tests) | Done |
| 4 | CLINT, PLIC stub, UART 16550 TX, programmatic DTB — OpenSBI banner | Done |
| 5 | Full PLIC/UART RX, F/D FP extension, Linux → BusyBox shell | Done |

## Build

```bash
cargo build
cargo build --release
```

Requires Rust 2021 edition (stable).

## Run

### Linux boot (requires images — see below)

```bash
cargo run --release -- \
  --dtb \
  --kernel  images/Image \
  --initrd  images/rootfs.img \
  images/fw_jump.elf
```

This loads OpenSBI (`fw_jump.elf`), the Linux kernel (`Image`), and a BusyBox initramfs (`rootfs.img`), builds a device tree programmatically, and drops into an interactive shell.

Add `--trace` to print every instruction to stderr.

### Bare ELF

```bash
cargo run -- path/to/binary.elf
cargo run -- --trace path/to/binary.elf
```

### Getting images

```bash
bash scripts/fetch-images.sh   # downloads Debian kernel + BusyBox initramfs
```

Requires `wget`. Places `fw_jump.elf`, `Image`, and `rootfs.img` in `images/` (gitignored).

## Test

```bash
cargo test                  # all 252 tests
cargo test rv64ui_p_        # RV64I subset
cargo test rv64um_p_        # M-extension
cargo test rv64ua_p_        # A-extension
cargo test rv64mi_p_        # M-mode privilege
cargo test rv64si_p_        # S-mode privilege
cargo test rv64ui_v_        # virtual-memory (Sv39)
```

The test suite runs vendored `riscv-tests` ELFs from `tests/riscv-tests/`.

## Architecture

```
src/main.rs          CLI, ELF loader, run loop, stdin thread, interrupt wiring
src/bus.rs           Address-mapped bus (RAM, CLINT, PLIC, UART)
src/loader.rs        ELF64 PT_LOAD loader
src/dtb.rs           Programmatic FDT builder (vm-fdt), mirrors QEMU virt
src/clint.rs         CLINT: mtime/mtimecmp, timer ticks
src/plic.rs          PLIC: SiFive-style, IRQ priority/enable/claim (S-mode context)
src/uart.rs          UART 16550: TX print, RX buffer, IER/IIR/LSR, DSR interception
src/cpu/mod.rs       Cpu struct, step(), privilege modes, trap delivery
src/cpu/decode.rs    Instruction enum + decode() for RV64GC
src/cpu/execute.rs   execute() — full RV64IMAFD + C + Zicsr
src/cpu/mmu.rs       Sv39 page walker, 64-entry direct-mapped TLB, A/D updates
src/cpu/csr.rs       M/S-mode CSRs, trap_entry/mret/sret helpers
```

## Memory map

Mirrors the QEMU `virt` machine.

| Device | Base address |
|--------|-------------|
| RAM | `0x8000_0000` (256 MiB) |
| CLINT | `0x0200_0000` |
| PLIC | `0x0c00_0000` |
| UART 16550 | `0x1000_0000` |
| virtio-blk MMIO | `0x1000_1000` (stub) |

## Resources

- [RISC-V Unprivileged Spec v20191213](https://riscv.org/technical/specifications/)
- [RISC-V Privileged Spec v20211203](https://riscv.org/technical/specifications/)
- [riscv-tests](https://github.com/riscv-software-src/riscv-tests)
- [OpenSBI](https://github.com/riscv-software-src/opensbi)
- [mini-rv32ima](https://github.com/cnlohr/mini-rv32ima) — minimal C reference
