use anyhow::{Context, Result};
use clap::Parser;
use riscv_emu::{cpu::{Cpu, PrivMode}, dtb, dtb::INITRD_BASE, loader, virtio_blk::VirtioBlk};
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

    /// Enable experimental x86-64 JIT compilation (faster but may produce wrong results)
    #[arg(long)]
    jit: bool,

    /// Enable emulator diagnostic messages on stderr (PC, mode, tick, JIT size, IRQ state)
    #[arg(long)]
    debug: bool,

    /// Enable verbose virtio-blk MMIO logging on stderr (every load/store + queue processing)
    #[arg(long)]
    virtio_debug: bool,

    /// Raw kernel Image to load at 0x8020_0000 (requires --dtb)
    #[arg(long)]
    kernel: Option<String>,

    /// Initramfs CPIO image to load at 0x8600_0000 (requires --dtb)
    #[arg(long)]
    initrd: Option<String>,

    /// Raw disk image to attach as virtio-blk /dev/vda (requires --dtb; disables initrd boot)
    #[arg(long)]
    disk: Option<String>,
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
    if args.disk.is_some() && !args.dtb {
        anyhow::bail!("--disk requires --dtb");
    }

    let mut initrd_size: u64 = 0;

    if let Some(ref kernel_path) = args.kernel {
        load_raw(&mut cpu.bus, kernel_path, KERNEL_BASE)?;
    }
    if let Some(ref initrd_path) = args.initrd {
        initrd_size = load_raw(&mut cpu.bus, initrd_path, INITRD_BASE)? as u64;
    }
    if let Some(ref disk_path) = args.disk {
        let file = std::fs::OpenOptions::new()
            .read(true).write(true)
            .open(disk_path)
            .with_context(|| format!("failed to open disk image {disk_path}"))?;
        let mut vblk = VirtioBlk::new(Some(file));
        vblk.debug = args.debug || args.virtio_debug;
        cpu.bus.virtio_blk = vblk;
    }

    let has_disk = args.disk.is_some();
    if args.dtb {
        let dtb_bytes = dtb::build_dtb(initrd_size, has_disk)
            .context("failed to build device tree")?;
        let dtb_off = (DTB_BASE - RAM_BASE) as usize;
        cpu.bus.ram_mut()[dtb_off..dtb_off + dtb_bytes.len()].copy_from_slice(&dtb_bytes);
        cpu.set_reg(10, 0);        // a0 = hart ID 0
        cpu.set_reg(11, DTB_BASE); // a1 = FDT pointer
    }

    // Stdin reader: forward raw bytes to the UART RX channel.
    // The UART already suppresses \033[6n in its TX path (intercepted before it
    // reaches the host terminal), so the host never sends a CPR response — no
    // need to filter \033[...R sequences here.
    // When stdin is not a TTY (piped input), mark the UART ready immediately.
    // The \033[6n terminal-probe mechanism only fires when bash/dash detects a
    // real TTY; piped input never triggers it, so bytes would be silently dropped.
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        cpu.bus.uart.stdin_ready = true;
    }

    let (stdin_tx, stdin_rx) = mpsc::channel::<u8>();
    std::thread::spawn(move || {
        use std::io::Read;
        let stdin = std::io::stdin();
        let mut buf = [0u8; 64];
        loop {
            match stdin.lock().read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for &b in &buf[..n] {
                        if stdin_tx.send(b).is_err() { return; }
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
    // Tick of last FENCE.I-triggered JIT flush. Rate-limited to avoid thrashing
    // during ftrace's 39K consecutive FENCE.I patch burst.
    let mut last_fence_i_flush: u64 = 0;
    // Wall-clock reference for advancing mtime at a fixed 10 MHz regardless of
    // emulation speed (interpreter vs JIT run at very different MIPS rates).
    let mut last_clint_tick = std::time::Instant::now();

    loop {
        // Scale tick by estimated instructions per JIT block so CLINT fires every
        // ~1024 guest instructions regardless of privilege mode:
        //   M-mode JIT blocks: up to 128 instructions → tick += 64 → fires every ~16 blocks
        //   S/U-mode JIT blocks: ~8 instructions avg  → tick += 8  → fires every 128 blocks
        //   Interpreter step: 1 instruction            → tick += 1  → fires every 1024 steps
        let tick_inc = if !args.jit {
            cpu.step()?;
            1
        } else {
            match jit.get(cpu.pc) {
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
                    if !cpu.jit_invalidate && !cpu.fence_i_pending {
                        jit.compile(&mut cpu, pc);
                    }
                    1
                }
            }
        };

        // Immediate flush for satp PPN changes (address-translation change).
        if cpu.jit_invalidate {
            cpu.jit_invalidate = false;
            jit.invalidate();
        }

        // Rate-limited flush for FENCE.I (instruction-cache fence after code patching).
        // ftrace issues ~39K FENCE.I calls in a burst; flushing on each would thrash
        // the JIT cache. Limit to one flush per 65536 ticks (~19ms at JIT speed) so
        // the burst only causes ~handful of flushes while still catching jump_label
        // patches that happen sparsely throughout boot.
        if cpu.fence_i_pending {
            cpu.fence_i_pending = false;
            if tick.wrapping_sub(last_fence_i_flush) >= 65536 {
                jit.invalidate();
                last_fence_i_flush = tick;
            }
        }

        tick = tick.wrapping_add(tick_inc);
        if tick & 1023 == 0 {
            // Push out any TX bytes still sitting in stdout's LineWriter buffer
            // (e.g. a prompt without a trailing '\n').  Cheap when nothing is
            // pending; runs once per 1024 instructions.
            cpu.bus.uart.flush_pending();
            // Drain stdin → UART RX only after the shell prompt is ready.
            if cpu.bus.uart.stdin_ready {
                while let Ok(byte) = stdin_rx.try_recv() {
                    cpu.bus.uart.push_rx(byte);
                }
            }
            // Advance mtime based on wall time at a 10 MHz timebase (100 ns/tick),
            // matching the RISC-V virt platform. This keeps timer accuracy constant
            // regardless of emulation speed.
            let now_c = std::time::Instant::now();
            let advance = now_c.duration_since(last_clint_tick).as_nanos() as u64 / 100;
            if advance > 0 {
                cpu.bus.clint.mtime = cpu.bus.clint.mtime.wrapping_add(advance);
                last_clint_tick = now_c;
            }
            if cpu.bus.clint.mtime >= cpu.bus.clint.mtimecmp {
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
            let virtio_irq = cpu.bus.virtio_blk.irq_status != 0;
            cpu.bus.plic.set_pending(1, virtio_irq);
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

            if args.debug {
                let now = std::time::Instant::now();
                if now.duration_since(last_diag).as_millis() >= 2000 {
                    last_diag = now;
                    let mode_str = match cpu.mode {
                        PrivMode::M => "M",
                        PrivMode::S => "S",
                        PrivMode::U => "U",
                    };
                    let moving = if cpu.pc != last_pc { "moving" } else { "STUCK" };
                    let mip = cpu.csr.mip;
                    let mie = cpu.csr.mie;
                    let sie  = (cpu.csr.mstatus >> 1) & 1;
                    let spie = (cpu.csr.mstatus >> 5) & 1;
                    let spp  = (cpu.csr.mstatus >> 8) & 1;
                    let mtime  = cpu.bus.clint.mtime;
                    let mtcmp  = cpu.bus.clint.mtimecmp;
                    let stcmp  = cpu.csr.stimecmp;
                    let dtm = mtcmp.wrapping_sub(mtime) as i64;
                    let dts = stcmp.wrapping_sub(mtime) as i64;
                    eprintln!("[emu] pc={:#018x} mode={} tick={:>12} jit={:>5}  {}  \
                               mip={:#06x} mie={:#06x} SIE={} SPIE={} SPP={}",
                              cpu.pc, mode_str, tick, jit.len(), moving,
                              mip, mie, sie, spie, spp);
                    eprintln!("[tmr] mtime={} mtimecmp={} stimecmp={} dt_m={} dt_s={}",
                              mtime, mtcmp,
                              if stcmp == u64::MAX { -1i64 as u64 } else { stcmp },
                              dtm, dts);
                    let rx_total  = cpu.bus.uart.rx_bytes;
                    let rx_drain  = cpu.bus.uart.rx_drained;
                    eprintln!("[uart] rx_total={} rx_drained={} rx_pending={} stdin_ready={} tx_total={}",
                              rx_total, rx_drain,
                              rx_total.wrapping_sub(rx_drain),
                              cpu.bus.uart.stdin_ready,
                              cpu.bus.uart.tx_bytes);
                    // virtio-blk state — to spot completions stuck unACKed (irq_status=1
                    // for many seconds) or notifies dropped (avail_idx > last_avail_idx).
                    let avail_addr = cpu.bus.virtio_blk.avail_addr;
                    let used_addr  = cpu.bus.virtio_blk.used_addr;
                    let q_ready    = cpu.bus.virtio_blk.queue_ready;
                    let avail_idx_ram = if q_ready != 0 && avail_addr != 0 {
                        cpu.bus.load(avail_addr.wrapping_add(2), 2).unwrap_or(0) as u16
                    } else { 0 };
                    let used_idx_ram = if q_ready != 0 && used_addr != 0 {
                        cpu.bus.load(used_addr.wrapping_add(2), 2).unwrap_or(0) as u16
                    } else { 0 };
                    let plic_pend = cpu.bus.plic.has_interrupt();
                    let vb = &cpu.bus.virtio_blk;
                    eprintln!("[vio] ready={} status={:#x} irq={} last_avail={} avail_idx_ram={} used_idx_ram={} plic_pend={}",
                              q_ready, vb.device_status, vb.irq_status,
                              vb.last_avail_idx, avail_idx_ram, used_idx_ram, plic_pend);
                    last_pc = cpu.pc;
                }
            }
        }
    }
}
