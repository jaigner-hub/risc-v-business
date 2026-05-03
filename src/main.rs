use anyhow::{Context, Result};
use clap::Parser;
use riscv_emu::{cpu::Cpu, loader};

#[derive(Parser)]
#[command(name = "riscv-emu", about = "RV64I emulator")]
struct Args {
    /// ELF binary to run
    binary: String,

    /// Enable per-instruction trace output on stderr
    #[arg(long)]
    trace: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let bytes = std::fs::read(&args.binary)
        .with_context(|| format!("failed to read {}", args.binary))?;

    let loaded = loader::load_elf(&bytes)?;
    let mut cpu = Cpu::new(loaded.bus, loaded.entry, args.trace);

    loop {
        cpu.step()?;
    }
}
