use anyhow::{Context, Result};
use clap::Parser;
use riscv_emu::{cpu::Cpu, dtb, dtb::INITRD_BASE, loader};

#[derive(Parser)]
#[command(name = "riscv-emu", about = "RV64I emulator")]
struct Args {
    /// ELF binary to run
    binary: String,

    /// Enable per-instruction trace output on stderr
    #[arg(long)]
    trace: bool,

    /// Build FDT programmatically and boot in OpenSBI mode (sets a0/a1, ticks CLINT)
    #[arg(long)]
    dtb: bool,

    /// Raw kernel Image to load at 0x8020_0000 (requires --dtb)
    #[arg(long)]
    kernel: Option<String>,

    /// Initramfs CPIO image to load at 0x8600_0000 (requires --dtb)
    #[arg(long)]
    initrd: Option<String>,
}

const DTB_BASE: u64 = 0x8220_0000;
const RAM_BASE: u64 = 0x8000_0000;
const KERNEL_BASE: u64 = 0x8020_0000;

fn load_raw(bus: &mut riscv_emu::bus::Bus, path: &str, phys_addr: u64) -> Result<usize> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read {path}"))?;
    let off = (phys_addr - RAM_BASE) as usize;
    let end = off.checked_add(bytes.len())
        .ok_or_else(|| anyhow::anyhow!("load overflows address space: {phys_addr:#x} + {} bytes", bytes.len()))?;
    if end > bus.ram_mut().len() {
        anyhow::bail!("{path}: file too large — {phys_addr:#x} + {} bytes exceeds RAM",
                      bytes.len());
    }
    bus.ram_mut()[off..end].copy_from_slice(&bytes);
    Ok(bytes.len())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let bytes = std::fs::read(&args.binary)
        .with_context(|| format!("failed to read {}", args.binary))?;

    let loaded = loader::load_elf(&bytes)?;
    let mut cpu = Cpu::new(loaded.bus, loaded.entry, args.trace);

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

    if args.dtb {
        let dtb_bytes = dtb::build_dtb(initrd_size)
            .context("failed to build device tree")?;
        let dtb_off = (DTB_BASE - RAM_BASE) as usize;
        cpu.bus.ram_mut()[dtb_off..dtb_off + dtb_bytes.len()].copy_from_slice(&dtb_bytes);
        cpu.set_reg(10, 0);        // a0 = hart ID 0
        cpu.set_reg(11, DTB_BASE); // a1 = FDT pointer
    }

    loop {
        cpu.step()?;
        if cpu.bus.clint.tick() {
            cpu.csr.mip |= 1 << 7;    // set MTIP
        } else {
            cpu.csr.mip &= !(1u64 << 7); // clear MTIP
        }
    }
}
