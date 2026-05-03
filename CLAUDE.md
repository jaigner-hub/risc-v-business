# RISC-V Emulator — Claude Notes

## Essential commands

```bash
cargo build                        # debug build
cargo build --release              # release build
cargo test                         # run all tests (includes riscv-tests harness)
cargo test rv64ui_p_               # run only Phase 1 ISA tests
cargo test bus                     # run Bus unit tests
cargo test cpu                     # run Cpu unit tests
cargo test decode                  # run decode unit tests
cargo test execute                 # run execute unit tests
cargo run -- <elf>                 # run an ELF binary
cargo run -- --trace <elf>         # run with per-instruction trace on stderr
```

## Project structure

```
src/lib.rs             pub mod re-exports (required for integration tests)
src/main.rs            CLI: clap args, load ELF, run loop
src/bus.rs             Bus: little-endian RAM load/store
src/loader.rs          ELF64: PT_LOAD segments + tohost symbol lookup
src/cpu/mod.rs         Cpu struct, step(), reg accessors, Tracer
src/cpu/decode.rs      Instruction enum + decode(u32) -> Result<Instruction>
src/cpu/execute.rs     execute(&mut Cpu, Instruction) -> Result<()>
build.rs               Generates one #[test] per vendored ELF in tests/riscv-tests/
tests/riscv_tests.rs   Includes build.rs output
tests/riscv-tests/     Vendored rv64ui-p-* ELF binaries (~40 files)
scripts/               One-time helper scripts
images/                OpenSBI, kernel Image, rootfs.img (gitignored)
```

## Phase gates

Do NOT start phase N+1 until phase N's test suite is fully green.

| Phase | Done criterion |
|-------|---------------|
| 1 (current) | All `rv64ui-p-*` pass |
| 2 | `rv64um-p-*`, `rv64ua-p-*`, `rv64mi-p-*` pass |
| 3 | `rv64si-p-*`, `rv64ui-v-*` pass |
| 4 | OpenSBI banner prints |
| 5 | `#` shell prompt, `uname -a` and `ls /` work |

## Memory map (mirrors QEMU virt)

| Device | Base |
|--------|------|
| RAM | `0x8000_0000` |
| CLINT | `0x0200_0000` |
| PLIC | `0x0c00_0000` |
| UART 16550 | `0x1000_0000` |
| virtio-blk MMIO | `0x1000_1000` |

## Adding instructions (Phase 2+)

1. Add variant to `Instruction` enum in `src/cpu/decode.rs`
2. Add decode arm in `decode()` — cite the spec section
3. Add execute arm in `execute()` in `src/cpu/execute.rs`
4. Wire up the corresponding `riscv-tests` suite in `build.rs`

## Vendoring new test ELFs

```bash
bash scripts/fetch-riscv-tests.sh   # builds from source (needs riscv64-unknown-elf-gcc)
git add tests/riscv-tests/
git commit -m "test: vendor <suite> ELFs"
```

## Debugging tips

- `--trace` prints `[pc] raw  mnemonic  operands  reg_changes` per step to stderr
- For kernel hangs in Phase 5, trace the Sv39 walker: every VA, satp, PTE, and final PA is logged when `--trace` is active
- Compare against QEMU: `qemu-system-riscv64 -d in_asm,cpu` dumps instructions and register state

## Spec references

- Unprivileged Spec v20191213 — instruction encoding, immediates, arithmetic semantics
- Privileged Spec v20211203 — CSRs, trap delivery, privilege modes, Sv39 MMU
