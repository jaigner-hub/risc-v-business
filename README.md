# risc-v-business

A RISC-V emulator in Rust, built to boot mainline Linux.

**Target:** RV64IMA, M/S/U privilege levels, Sv39 MMU, OpenSBI + Linux + BusyBox initramfs.

**Status:** Phase 1 in progress — RV64I user-mode interpreter.

## Phases

| # | Scope | Status |
|---|-------|--------|
| 1 | RV64I interpreter, riscv-tests passing | In progress |
| 2 | M/A extensions, Zicsr, privilege modes, traps | Not started |
| 3 | Sv39 MMU, TLB | Not started |
| 4 | CLINT, PLIC, UART, virtio-blk | Not started |
| 5 | Boot Linux to `#` prompt | Not started |

## Build

```bash
cargo build
cargo build --release
```

Requires Rust 2021 edition (stable).

## Run

```bash
cargo run -- path/to/binary.elf
cargo run -- --trace path/to/binary.elf   # per-instruction trace on stderr
```

## Test

```bash
cargo test
```

The test suite runs vendored `riscv-tests` ELFs from `tests/riscv-tests/`. If the directory is empty, build them first:

```bash
# Requires: riscv64-unknown-elf-gcc, autoconf, make
# Ubuntu/Debian: sudo apt install gcc-riscv64-unknown-elf autoconf
bash scripts/fetch-riscv-tests.sh
```

## Memory map

Mirrors the QEMU `virt` machine so QEMU's DTB can be reused without modification.

| Device | Base address |
|--------|-------------|
| RAM | `0x8000_0000` |
| CLINT | `0x0200_0000` |
| PLIC | `0x0c00_0000` |
| UART 16550 | `0x1000_0000` |
| virtio-blk MMIO | `0x1000_1000` |

## Resources

- [Writing a RISC-V Emulator in Rust](https://book.rvemu.app)
- [mini-rv32ima](https://github.com/cnlohr/mini-rv32ima) — minimal C reference that boots Linux
- [RISC-V specs](https://riscv.org/technical/specifications/) — Unprivileged + Privileged
- [riscv-tests](https://github.com/riscv-software-src/riscv-tests)
- [OpenSBI](https://github.com/riscv-software-src/opensbi)
- [Buildroot](https://buildroot.org/)
