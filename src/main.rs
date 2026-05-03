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
