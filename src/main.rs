use anyhow::{Context, Result};
use clap::Parser;
use riscv_emu::{cpu::{Cpu, PrivMode}, dtb, dtb::INITRD_BASE, loader};
use std::sync::mpsc;

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

    // Stdin reader thread feeds bytes into the UART RX buffer.
    // Filters ANSI CPR sequences (\033[<digits>;<digits>R) which the host terminal
    // sends in response to our \033[6n cursor-position query; the emulator already
    // injects a synthetic \033[1;1R, so the real host response must be discarded.
    // Other ESC sequences (arrow keys, function keys) are buffered until we know
    // whether they're CPR, then replayed if they're not.
    let (stdin_tx, stdin_rx) = mpsc::channel::<u8>();
    std::thread::spawn(move || {
        use std::io::Read;
        let stdin = std::io::stdin();
        let mut buf = [0u8; 64];
        let mut esc_buf: Vec<u8> = Vec::new();
        loop {
            match stdin.lock().read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for &b in &buf[..n] {
                        if esc_buf.is_empty() {
                            if b == 0x1B {
                                esc_buf.push(b);
                            } else if stdin_tx.send(b).is_err() {
                                return;
                            }
                        } else {
                            match esc_buf.len() {
                                1 => {
                                    if b == b'[' {
                                        esc_buf.push(b);
                                    } else {
                                        for &c in &esc_buf { let _ = stdin_tx.send(c); }
                                        esc_buf.clear();
                                        if stdin_tx.send(b).is_err() { return; }
                                    }
                                },
                                _ => {
                                    if b.is_ascii_digit() || b == b';' {
                                        esc_buf.push(b);
                                    } else if b == b'R' && esc_buf.len() >= 3 {
                                        esc_buf.clear();
                                    } else {
                                        for &c in &esc_buf { let _ = stdin_tx.send(c); }
                                        esc_buf.clear();
                                        if stdin_tx.send(b).is_err() { return; }
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    });

    use riscv_emu::jit::JitCache;
    let mut jit = JitCache::new();
    let mut tick: u64 = 0;
    let mut last_diag = std::time::Instant::now();
    let mut last_pc: u64 = 0;

    loop {
        // Scale tick by estimated instructions per JIT block so CLINT fires every
        // ~1024 guest instructions regardless of privilege mode:
        //   M-mode JIT blocks: up to 128 instructions → tick += 64 → fires every ~16 blocks
        //   S/U-mode JIT blocks: ~8 instructions avg  → tick += 8  → fires every 128 blocks
        //   Interpreter step: 1 instruction            → tick += 1  → fires every 1024 steps
        let tick_inc = match jit.get(cpu.pc) {
            Some(f) => {
                let mode = cpu.mode;
                let next = unsafe { f(cpu.regs.as_mut_ptr(), &mut cpu as *mut Cpu) };
                if next == u64::MAX {
                    cpu.step()?;
                    1
                } else {
                    cpu.pc = next;
                    if mode == PrivMode::M { 64 } else { 8 }
                }
            }
            None => {
                let pc = cpu.pc;
                cpu.step()?;
                if !cpu.jit_invalidate {
                    jit.compile(&mut cpu, pc);
                }
                1
            }
        };

        if cpu.jit_invalidate {
            cpu.jit_invalidate = false;
            jit.invalidate();
        }

        tick = tick.wrapping_add(tick_inc);
        if tick & 1023 == 0 {
            // Drain stdin → UART RX only after the shell prompt is ready.
            if cpu.bus.uart.stdin_ready {
                while let Ok(byte) = stdin_rx.try_recv() {
                    cpu.bus.uart.push_rx(byte);
                }
            }
            if cpu.bus.clint.tick() {
                cpu.csr.mip |= 1 << 7;
            } else {
                cpu.csr.mip &= !(1u64 << 7);
            }
            // Sstc extension: only manage STIP from stimecmp when the kernel has
            // explicitly written stimecmp (≠ default u64::MAX). Without Sstc the
            // kernel uses SBI SET_TIMER → OpenSBI injects STIP via mip write; the
            // unconditional 'else' branch was clearing that injection every batch.
            if cpu.csr.stimecmp != u64::MAX {
                if cpu.bus.clint.mtime >= cpu.csr.stimecmp {
                    cpu.csr.mip |= 1 << 5;
                } else {
                    cpu.csr.mip &= !(1u64 << 5);
                }
            }
            let uart_irq = cpu.bus.uart.irq_pending();
            cpu.bus.plic.set_pending(10, uart_irq);
            if cpu.bus.plic.has_interrupt() {
                cpu.csr.mip |= 1 << 9;
            } else {
                cpu.csr.mip &= !(1u64 << 9);
            }

            // Deliver any pending interrupts now that mip is fully updated.
            // JIT blocks execute without calling step(), so interrupts are never
            // checked inside tight JIT loops (e.g. the WFI idle loop). Without this
            // call the CPU would spin in the idle loop forever after mip is raised.
            cpu.check_interrupts();

            // Emit a diagnostic line to stderr every 5 seconds of wall time.
            // Shows whether the emulator is making forward progress (PC moving)
            // or is stuck in a tight loop, and which code range is hot.
            let now = std::time::Instant::now();
            if now.duration_since(last_diag).as_secs() >= 5 {
                last_diag = now;
                let mode_str = match cpu.mode {
                    PrivMode::M => "M",
                    PrivMode::S => "S",
                    PrivMode::U => "U",
                };
                let moving = if cpu.pc != last_pc { "moving" } else { "STUCK" };
                eprintln!("[emu] pc={:#018x} mode={} tick={:>12} jit_blocks={:>5}  {}",
                          cpu.pc, mode_str, tick, jit.len(), moving);
                last_pc = cpu.pc;
            }
        }
    }
}
