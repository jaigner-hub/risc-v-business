use anyhow::Result;
use vm_fdt::FdtWriter;

pub const INITRD_BASE: u64 = 0x8600_0000;

/// Build a Flattened Device Tree blob for the QEMU virt-like RISC-V machine.
///
/// If `initrd_size > 0`, the `linux,initrd-start` and `linux,initrd-end` properties
/// are added to the `chosen` node so the kernel can find the initramfs.
pub fn build_dtb(initrd_size: u64) -> Result<Vec<u8>> {
    let mut fdt = FdtWriter::new()?;

    // Root node
    let root = fdt.begin_node("")?;
    fdt.property_u32("#address-cells", 2)?;
    fdt.property_u32("#size-cells", 2)?;
    fdt.property_string("compatible", "riscv-virtio")?;
    fdt.property_string("model", "riscv-virtio,qemu")?;

    // chosen
    let chosen = fdt.begin_node("chosen")?;
    fdt.property_string("stdout-path", "/soc/serial@10000000")?;
    fdt.property_string(
        "bootargs",
        "console=ttyS0 earlycon=uart8250,mmio,0x10000000 rdinit=/init",
    )?;
    if initrd_size > 0 {
        fdt.property_u64("linux,initrd-start", INITRD_BASE)?;
        fdt.property_u64("linux,initrd-end", INITRD_BASE + initrd_size)?;
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

    // cpu interrupt-controller (phandle 1)
    let intc = fdt.begin_node("interrupt-controller")?;
    fdt.property_phandle(1)?;
    fdt.property_u32("#interrupt-cells", 1)?;
    fdt.property_string("compatible", "riscv,cpu-intc")?;
    fdt.property_null("interrupt-controller")?;
    fdt.end_node(intc)?;

    fdt.end_node(cpu0)?;
    fdt.end_node(cpus)?;

    // memory@80000000 — 256 MiB starting at 0x8000_0000
    let memory = fdt.begin_node("memory@80000000")?;
    fdt.property_string("device_type", "memory")?;
    // #address-cells=2, #size-cells=2 → 4 u32 cells: addr_hi, addr_lo, size_hi, size_lo
    fdt.property_array_u32("reg", &[0x0000_0000, 0x8000_0000, 0x0000_0000, 0x1000_0000])?;
    fdt.end_node(memory)?;

    // soc
    let soc = fdt.begin_node("soc")?;
    fdt.property_u32("#address-cells", 2)?;
    fdt.property_u32("#size-cells", 2)?;
    fdt.property_string("compatible", "simple-bus")?;
    fdt.property_null("ranges")?;

    // clint@2000000
    let clint = fdt.begin_node("clint@2000000")?;
    fdt.property_string("compatible", "riscv,clint0")?;
    // interrupts-extended: phandle=1 irq=3 (MSIP), phandle=1 irq=7 (MTIP)
    fdt.property_array_u32("interrupts-extended", &[1, 3, 1, 7])?;
    fdt.property_array_u32("reg", &[0x0000_0000, 0x0200_0000, 0x0000_0000, 0x0001_0000])?;
    fdt.end_node(clint)?;

    // plic@c000000 (phandle 2)
    let plic = fdt.begin_node("plic@c000000")?;
    fdt.property_string("compatible", "sifive,plic-1.0.0")?;
    fdt.property_phandle(2)?;
    fdt.property_u32("#interrupt-cells", 1)?;
    fdt.property_null("interrupt-controller")?;
    // interrupts-extended: phandle=1 irq=11 (MEIP), phandle=1 irq=9 (SEIP)
    fdt.property_array_u32("interrupts-extended", &[1, 11, 1, 9])?;
    fdt.property_array_u32("reg", &[0x0000_0000, 0x0C00_0000, 0x0000_0000, 0x0400_0000])?;
    fdt.property_u32("riscv,ndev", 31)?;
    fdt.end_node(plic)?;

    // serial@10000000
    let serial = fdt.begin_node("serial@10000000")?;
    fdt.property_string("compatible", "ns16550a")?;
    fdt.property_array_u32("reg", &[0x0000_0000, 0x1000_0000, 0x0000_0000, 0x0000_0100])?;
    fdt.property_u32("clock-frequency", 3_686_400)?;
    fdt.property_u32("interrupts", 10)?;
    fdt.property_u32("interrupt-parent", 2)?;
    fdt.end_node(serial)?;

    fdt.end_node(soc)?;
    fdt.end_node(root)?;

    Ok(fdt.finish()?)
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
        assert!(
            bytes.windows(18).any(|w| w == b"linux,initrd-start"),
            "expected linux,initrd-start property in DTB"
        );
    }

    #[test]
    fn build_dtb_without_initrd_omits_initrd_properties() {
        let bytes = build_dtb(0).unwrap();
        assert!(
            !bytes.windows(18).any(|w| w == b"linux,initrd-start"),
            "linux,initrd-start should not appear when initrd_size=0"
        );
    }
}
