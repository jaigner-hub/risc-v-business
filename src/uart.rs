use std::collections::VecDeque;
use std::io::{self, Write};

pub struct Uart16550 {
    ier: u8,
    rx_buf: VecDeque<u8>,
    // Intercept every \033[6n (ANSI DSR cursor query) before it reaches the host
    // terminal — suppress the bytes, inject \033[1;1R into rx_buf.  Active for the
    // entire session because the shell re-sends DSR before every prompt.
    dsr_state: u8,
    pub stdin_ready: bool,  // true after first DSR — safe to inject stdin bytes
}

impl Uart16550 {
    pub fn new() -> Self {
        Self {
            ier: 0,
            rx_buf: VecDeque::new(),
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

        // Intercept \033[6n (ANSI DSR cursor query) before printing — the host terminal
        // must never see it or it sends a real CPR that leaks into the shell as input.
        // Active for the entire session: the shell re-sends DSR before every prompt.
        match (self.dsr_state, byte) {
            (0, 0x1B) => { self.dsr_state = 1; return; }
            (1, b'[') => { self.dsr_state = 2; return; }
            (2, b'6') => { self.dsr_state = 3; return; }
            (3, b'n') => {
                for &b in b"\x1b[1;1R" { self.rx_buf.push_back(b); }
                self.stdin_ready = true;
                self.dsr_state = 0;
                return;
            }
            // Sequence broke — flush buffered prefix then fall through to print byte
            (1, _) => { print!("\x1b"); self.dsr_state = 0; }
            (2, _) => { print!("\x1b["); self.dsr_state = 0; }
            (3, _) => { print!("\x1b[6"); self.dsr_state = 0; }
            _ => {}
        }

        print!("{}", byte as char);
        let _ = io::stdout().flush();
    }
}
