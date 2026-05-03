# RISC-V Emulator Phase 4 Design: OpenSBI Banner

## Goal

Add enough device support for OpenSBI to initialize and print its boot banner. Done criterion: running `cargo run -- images/fw_jump.elf --dtb` (with VIRT_DTB embedded) prints the OpenSBI banner to stdout.

## Architecture

Bus gains three named device fields. `bus.load` / `bus.store` dispatch by address range before falling through to RAM. Each device lives in its own source file. A committed DTB blob is embedded in the binary.

### New files

```
src/clint.rs               CLINT: mtime counter + mtimecmp register
src/uart.rs                UART 16550 stub: TX-ready + char output to stdout
src/plic.rs                PLIC stub: reads 0, drops writes
src/dtb.rs                 pub const VIRT_DTB: &[u8] = include_bytes!("../dtb/virt.dtb")
dtb/virt.dts               minimal device tree source (committed)
dtb/virt.dtb               compiled DTB binary (committed)
scripts/fetch-opensbi.sh   downloads fw_jump.elf from OpenSBI GitHub releases
images/                    gitignored runtime directory; holds fw_jump.elf
```

### Modified files

```
src/bus.rs     add clint/uart/plic fields + address dispatch
src/lib.rs     pub mod clint, uart, plic, dtb
src/main.rs    add --dtb boolean flag (use embedded VIRT_DTB), set a0/a1, tick CLINT per step
```

## Memory map

| Device  | Base           | Size      |
|---------|----------------|-----------|
| PLIC    | `0x0C00_0000`  | 64 MiB    |
| CLINT   | `0x0200_0000`  | 64 KiB    |
| UART    | `0x1000_0000`  | 256 B     |
| RAM     | `0x8000_0000`  | 128 MiB   |
| DTB     | `0x8220_0000`  | (in RAM)  |

OpenSBI loads at `0x8000_0000` (fw_jump.elf entry). Kernel jump target is `0x8020_0000` (hardcoded in fw_jump generic platform) — nothing is loaded there in Phase 4; the fault after the banner is acceptable.

## Device specifications

### CLINT (`0x0200_0000–0x0200_FFFF`)

```
offset 0xBFF8  mtime       u64  read-only; incremented by 1 each cpu.step()
offset 0x4000  mtimecmp[0] u64  read/write; stored but not acted on in Phase 4
```

`Clint::tick()` is called once per step from the main run loop. Timer interrupt delivery (mip.MTIP when mtime ≥ mtimecmp) is deferred to Phase 5.

### UART 16550 (`0x1000_0000–0x1000_00FF`)

OpenSBI uses ns16550a driver. Only two registers matter for output:

```
offset 0  THR  write: print char to stdout, flush
offset 5  LSR  read:  always 0x60 (bits 5+6 = TX empty + THR empty)
```

All other offsets: reads return 0, writes are no-ops. DLAB multiplexing is ignored — OpenSBI only probes LSR before writing THR.

### PLIC (`0x0C00_0000–0x0FFF_FFFF`)

Pure stub. All reads return 0. All writes are silently dropped. OpenSBI initializes the PLIC during boot but does not require it to function to reach the banner.

## Device tree (DTB)

OpenSBI's generic platform reads the FDT to locate the UART (`chosen.stdout-path`). The minimal DTS:

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

Compiled with `dtc -I dts -O dtb -o dtb/virt.dtb dtb/virt.dts` and committed. `src/dtb.rs` embeds it:

```rust
pub const VIRT_DTB: &[u8] = include_bytes!("../dtb/virt.dtb");
```

## Boot flow

`scripts/fetch-opensbi.sh` downloads `fw_jump.elf` (OpenSBI v1.5, generic platform) from GitHub releases into `images/`.

`main.rs` when `--dtb` flag is present:

1. Load `fw_jump.elf` via existing `loader::load_elf()` — PT_LOAD segment lands at `0x8000_0000`
2. Copy `dtb::VIRT_DTB` into RAM at `0x8220_0000`
3. `cpu.set_reg(10, 0)` — a0 = hart ID 0 (OpenSBI calling convention from prior M-mode stage)
4. `cpu.set_reg(11, 0x8220_0000)` — a1 = FDT pointer
5. Run loop: each iteration calls `cpu.step()` then `cpu.bus.clint.tick()`

UART writes appear on stdout as OpenSBI initializes. After the banner, OpenSBI jumps to `0x8020_0000`; the instruction fetch faults and the emulator traps/panics — acceptable for Phase 4.

## Compatibility

The existing riscv-tests path is unchanged. `--dtb` is an optional flag; without it `main.rs` behaves exactly as before. All 164 tests must continue to pass.

## Testing

No new automated tests for the devices (they are stubs). Manual verification:

```bash
bash scripts/fetch-opensbi.sh
cargo run -- images/fw_jump.elf --dtb
# Expected: OpenSBI banner appears on stdout, then emulator crashes/faults
```

The Phase 4 gate is cleared when the banner is visible.
