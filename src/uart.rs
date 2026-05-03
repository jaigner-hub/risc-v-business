use std::collections::VecDeque;
use std::io::{self, Write};

pub struct Uart16550 {
    ier: u8,
    rx_buf: VecDeque<u8>,
    // Two-phase detection in TX output:
    //   phase 1 — wait for "# " (shell prompt); prevents bytes arriving during kernel boot
    //   phase 2 — detect and auto-respond to \033[6n (ANSI DSR cursor query);
    //              prevents readline consuming stdin bytes while waiting for cursor position
    prompt_tail: u8,
    dsr_state: u8,
    pub stdin_ready: bool,  // true once phase 2 is complete — safe to inject stdin bytes
}

impl Uart16550 {
    pub fn new() -> Self {
        Self {
            ier: 0,
            rx_buf: VecDeque::new(),
            prompt_tail: 0,
            dsr_state: 0,
            stdin_ready: false,
        }
    }

    pub fn push_rx(&mut self, byte: u8) {
        self.rx_buf.push_back(byte);
    }

    fn data_ready(&self) -> bool {
        !self.rx_buf.is_empty()
    }

    /// Interrupt pending when THRE (IER[1]) or RX data ready (IER[0] + DR).
    pub fn irq_pending(&self) -> bool {
        (self.ier & 0x02) != 0 || ((self.ier & 0x01) != 0 && self.data_ready())
    }

    pub fn load(&mut self, addr: u64, _width: usize) -> u64 {
        match addr & 0xFF {
            0 => self.rx_buf.pop_front().unwrap_or(0) as u64,
            1 => self.ier as u64,
            2 => {
                if (self.ier & 0x01) != 0 && self.data_ready() {
                    0x04  // RXD available
                } else if (self.ier & 0x02) != 0 {
                    0x02  // THRE
                } else {
                    0x01  // no interrupt pending
                }
            }
            5 => {
                let mut lsr = 0x60u64;
                if self.data_ready() { lsr |= 0x01; }
                lsr
            }
            6 => 0x30,
            _ => 0,
        }
    }

    pub fn store(&mut self, addr: u64, _width: usize, val: u64) {
        if addr & 0xFF != 0 {
            if addr & 0xFF == 1 { self.ier = val as u8; }
            return;
        }
        let byte = val as u8;
        print!("{}", byte as char);
        let _ = io::stdout().flush();

        if self.stdin_ready {
            return;
        }

        // Phase 1: wait for "# " (shell prompt ready).
        // Phase 2: watch for \033[6n (ANSI DSR query) and auto-respond so readline
        //          gets a valid cursor position before we inject any user input.
        match self.prompt_tail {
            0 | 1 => {
                self.prompt_tail = if byte == b'#' { 1 } else if self.prompt_tail == 1 && byte == b' ' { 2 } else { 0 };
            }
            _ => {
                // Prompt seen — now wait for \033[6n
                self.dsr_state = match (self.dsr_state, byte) {
                    (0, 0x1B) => 1,
                    (1, b'[') => 2,
                    (2, b'6') => 3,
                    (3, b'n') => {
                        // Respond with ESC [ 1 ; 1 R before any stdin bytes arrive
                        for &b in b"\x1b[1;1R" {
                            self.rx_buf.push_back(b);
                        }
                        self.stdin_ready = true;
                        0
                    }
                    _ => 0,
                };
            }
        }
    }
}
