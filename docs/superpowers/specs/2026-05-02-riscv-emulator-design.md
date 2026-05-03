# RISC-V Emulator Design

**Date:** 2026-05-02  
**Language:** Rust  
**Goal:** Boot mainline Linux with a BusyBox initramfs to a `#` shell prompt.

---

## Target configuration

| Dimension | Value |
|-----------|-------|
| ISA | RV64IMA (no F/D/C until after Linux boots) |
| Privilege levels | M, S, U |
| MMU | Sv39 (three-level, 39-bit VA) |
| Firmware | OpenSBI `fw_jump.bin` (prebuilt) |
| Kernel | Linux mainline, `arch/riscv` defconfig, `CONFIG_ARCH_RV64I=y` |
| Userspace | BusyBox initramfs via Buildroot `qemu_riscv64_virt_defconfig` |
| Memory map | Mirrors QEMU `virt` (see below) |

### Memory map

| Device | Base address |
|--------|-------------|
| RAM | `0x8000_0000` |
| CLINT | `0x0200_0000` |
| PLIC | `0x0c00_0000` |
| UART 16550 | `0x1000_0000` |
| virtio-blk MMIO | `0x1000_1000` |

Matching QEMU `virt` exactly allows reuse of QEMU's DTB without modification.

---

## Repository layout

```
riscv-emu/
├── Cargo.toml
├── src/
│   ├── main.rs           # arg parsing, top-level loop
│   ├── cpu/
│   │   ├── mod.rs        # Cpu struct, step()
│   │   ├── decode.rs     # instruction decode → enum
│   │   ├── execute.rs    # per-instruction handlers
│   │   ├── csr.rs        # CSR file + privilege checks
│   │   └── trap.rs       # trap/interrupt delivery
│   ├── mmu.rs            # Sv39 page walker + TLB
│   ├── bus.rs            # memory + MMIO dispatch
│   ├── devices/
│   │   ├── clint.rs
│   │   ├── plic.rs
│   │   ├── uart.rs
│   │   └── virtio_blk.rs
│   └── loader.rs         # ELF + raw binary loaders
├── tests/
│   ├── riscv_tests.rs    # harness for official test suite
│   └── riscv-tests/      # vendored rv64ui-p-* ELF binaries (~500 KB)
└── images/               # OpenSBI, kernel Image, rootfs.img (gitignored)
```

---

## Core types

### `Cpu`

```rust
pub struct Cpu {
    regs: [u64; 32],   // regs[0] always returns 0 via read accessor
    pc:   u64,
    bus:  Bus,
    tracer: Tracer,
}
```

- `reg_read(n: usize) -> u64`: returns 0 for n=0, `regs[n]` otherwise.
- `reg_write(n: usize, val: u64)`: writes silently ignored for n=0.
- `step(&mut self) -> Result<()>`: fetch 4 bytes at `pc` → `decode` → `execute` → advance `pc`.

### `Bus`

```rust
pub struct Bus {
    ram:      Vec<u8>,
    ram_base: u64,
}
```

- `load(addr: u64, width: usize) -> Result<u64>`: little-endian, width ∈ {1,2,4,8}.
- `store(addr: u64, width: usize, value: u64) -> Result<()>`: little-endian.
- Phase 1: RAM only. MMIO ranges added in Phase 4.
- The `tohost` address (resolved from the test ELF's symbol table by the test harness) is intercepted by the harness, not inside `Bus`.

---

## Instruction decode

`decode(inst: u32) -> Result<Instruction>` returns an enum with one variant per RV64I instruction (47 variants in Phase 1). Each variant carries decoded, sign-extended fields — `execute` never touches raw instruction bits.

Decode hierarchy follows the spec encoding tables:

```rust
match opcode {
    0b0110011 => match (funct3, funct7) { /* R-type */ }
    0b0010011 => match funct3 { /* I-type arithmetic */ }
    0b0000011 => match funct3 { /* loads */ }
    0b0100011 => match funct3 { /* stores */ }
    0b1100011 => match funct3 { /* branches */ }
    0b1101111 => /* JAL */
    0b1100111 => /* JALR */
    0b0110111 => /* LUI */
    0b0010111 => /* AUIPC */
    0b0001111 => /* FENCE */
    0b1110011 => /* ECALL / EBREAK */
    // *W 32-bit variants (0b0111011, 0b0011011)
    _ => Err(IllegalInstruction(inst))
}
```

`sext32(val: u64) -> u64` is a shared helper for the `*W` variants — sign extending from bit 31 is the most common bug in this family.

Spec references in comments for every non-obvious decode behavior (Unprivileged Spec v20191213).

---

## Testing strategy

`rv64ui-p-*` ELF binaries are **vendored** in `tests/riscv-tests/` (committed to the repo, ~500 KB). No RISC-V toolchain required to run tests — `cargo test` works on any machine.

### Harness (`tests/riscv_tests.rs`)

For each `rv64ui-p-*` file:

1. Load the ELF into a fresh `Cpu` via `loader::load_elf`.
2. Resolve the `tohost` symbol address from the ELF symbol table.
3. Run `cpu.step()` in a loop, watching for a store to `tohost`.
4. Value `1` at `tohost` → pass. Any other value, or no `tohost` write within 100 million steps, → fail.
5. Test is identified by ELF filename in failure output.

One `#[test]` per ELF, generated via a `build.rs` build script that scans `tests/riscv-tests/` at compile time.

Phase 2 adds `rv64um-p-*`, `rv64ua-p-*`, `rv64mi-p-*`. Phase 3 adds `rv64si-p-*` and `rv64ui-v-*`.

---

## Tracing / observability

A `--trace` CLI flag enables per-step output to stderr:

```
[0x80000000] 00000517  auipc  x10, 0x0       x10: 0x0000000000000000 -> 0x0000000080000000
```

Format: `[pc] raw_hex  mnemonic  operands    changed_reg: old -> new`

Only registers that change are printed per step. Implemented as a `Tracer` struct held in `Cpu`; when `--trace` is not set the tracer is a no-op, costing one branch per step (negligible at interpreter speeds). No `log`/`tracing` crate dependency in Phase 1; `eprintln!` is sufficient.

---

## Dependencies (Phase 1)

- `clap` — CLI argument parsing (`--trace`, binary path)
- `anyhow` — error propagation
- `goblin` — ELF64 parsing (PT_LOAD segments, symbol table)

No other dependencies until Phase 4+ (peripherals may add `crossterm` or similar for UART I/O).

---

## Phases

Each phase is gated: do not start phase N+1 until phase N's done criterion is green.

| Phase | Scope | Done criterion |
|-------|-------|----------------|
| 1 | RV64I user-mode interpreter | All `rv64ui-p-*` tests pass |
| 2 | M, A, Zicsr, privilege modes, traps | `rv64um-p-*`, `rv64ua-p-*`, `rv64mi-p-*` pass |
| 3 | Sv39 MMU + TLB | `rv64si-p-*`, `rv64ui-v-*` pass |
| 4 | CLINT, PLIC, UART 16550, virtio-blk | OpenSBI prints its banner |
| 5 | Boot Linux | `#` prompt, `uname -a` works, `ls /` works |
| 6+ | C/F/D ext, SMP, JIT, GDB stub | After Phase 5 ships |

---

## Coding guidelines

- **Spec-driven:** cite Unprivileged Spec v20191213 / Privileged Spec v20211203 section in comments for every non-obvious behavior.
- **Tests are the spec:** `riscv-tests` is the source of truth. Run on every change.
- **No premature optimization:** match interpreter running ~10–50 MIPS is sufficient to boot Linux. Optimize after Phase 5.
- **Sv39 debug mode:** page walker logs every step (VA, satp, each PTE, final PA or fault cause) when `--trace` is active. Essential for Phase 5 kernel hang debugging.
- **`#![deny(unsafe_op_in_unsafe_fn)]`** project-wide.

---

## Resources

- [Writing a RISC-V Emulator in Rust](https://book.rvemu.app) — Phases 1–4 in Rust
- [mini-rv32ima](https://github.com/cnlohr/mini-rv32ima) — ~400 lines of C that boots Linux
- [TinyEMU](https://bellard.org/tinyemu/) — Bellard's reference implementation
- [RISC-V specs](https://riscv.org/technical/specifications/) — Unprivileged + Privileged (mandatory reading)
- [riscv-tests](https://github.com/riscv-software-src/riscv-tests) — official compliance suite
- [OpenSBI](https://github.com/riscv-software-src/opensbi) — `make PLATFORM=generic` → `fw_jump.bin`
- [Buildroot](https://buildroot.org/) — `make qemu_riscv64_virt_defconfig && make`
