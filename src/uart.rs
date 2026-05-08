use std::collections::VecDeque;
use std::io::{self, Write};

pub struct Uart16550 {
    ier: u8,
    rx_buf: VecDeque<u8>,
    /// State machine for suppressing \033[6n (ANSI cursor-position query) from
    /// reaching the host terminal.  0=idle, 1=saw ESC, 2=saw ESC[, 3=saw ESC[6.
    dsr_state: u8,
    /// Match progress through "\x1b[?2004h" (bash bracketed-paste-mode enable).
    /// With TERM=dumb bash never issues \033[6n, so we use this sequence as
    /// the fallback signal that the shell is up and ready to receive stdin.
    bpm_state: u8,
    /// Set once the shell has signaled readiness (via \033[6n DSR query OR
    /// \033[?2004h bracketed-paste enable). Gates stdin injection so bytes
    /// don't reach the guest before it is ready.
    pub stdin_ready: bool,
    /// Total bytes emitted to host stdout (post-DSR-suppression). Diagnostic only.
    pub tx_bytes: u64,
    /// Total bytes pushed into RX buffer (from stdin). Diagnostic only.
    pub rx_bytes: u64,
    /// Total bytes drained out of RX buffer by guest (load @0x0). Diagnostic only.
    pub rx_drained: u64,
}

impl Uart16550 {
    pub fn new() -> Self {
        Self {
            ier: 0,
            rx_buf: VecDeque::new(),
            dsr_state: 0,
            bpm_state: 0,
            stdin_ready: false,
            tx_bytes: 0,
            rx_bytes: 0,
            rx_drained: 0,
        }
    }

    pub fn push_rx(&mut self, byte: u8) {
        self.rx_buf.push_back(byte);
        self.rx_bytes = self.rx_bytes.wrapping_add(1);
    }

    fn data_ready(&self) -> bool { !self.rx_buf.is_empty() }

    pub fn irq_pending(&self) -> bool {
        (self.ier & 0x02) != 0 || ((self.ier & 0x01) != 0 && self.data_ready())
    }

    pub fn load(&mut self, addr: u64, _width: usize) -> u64 {
        match addr & 0xFF {
            0 => {
                let b = self.rx_buf.pop_front().unwrap_or(0);
                if !self.rx_buf.is_empty() || b != 0 { self.rx_drained = self.rx_drained.wrapping_add(1); }
                b as u64
            }
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

    /// Force any buffered TX out to the host terminal.  Cheap to call repeatedly
    /// when nothing is buffered; meant to be invoked from the main loop's
    /// stdin/timer poll so prompts (which have no trailing '\n' and therefore
    /// don't trigger LineWriter's auto-flush) become visible promptly.
    pub fn flush_pending(&self) {
        let _ = io::stdout().flush();
    }

    /// Output any bytes held while matching \033[6n, then reset state to 0.
    fn flush_prefix(&mut self) {
        let prefix: &[u8] = match self.dsr_state {
            1 => b"\x1b",
            2 => b"\x1b[",
            3 => b"\x1b[6",
            _ => return,
        };
        for &h in prefix { print!("{}", h as char); }
        self.tx_bytes = self.tx_bytes.wrapping_add(prefix.len() as u64);
        self.dsr_state = 0;
    }

    pub fn store(&mut self, addr: u64, _width: usize, val: u64) {
        if addr & 0xFF != 0 {
            if addr & 0xFF == 1 { self.ier = val as u8; }
            return;
        }
        let byte = val as u8;

        // Watch for "\x1b[?2004h" (bash bracketed-paste-mode enable) as a
        // fallback gate-opener.  Runs in parallel with the DSR detector below;
        // these sequences share the "\x1b[" prefix but diverge at the third
        // byte ('?' vs '6'), so the two state machines never collide.
        const BPM_TARGET: &[u8] = b"\x1b[?2004h";
        let bpm_idx = self.bpm_state as usize;
        if bpm_idx < BPM_TARGET.len() && byte == BPM_TARGET[bpm_idx] {
            self.bpm_state += 1;
            if (self.bpm_state as usize) == BPM_TARGET.len() {
                self.stdin_ready = true;
                self.bpm_state = 0;
            }
        } else {
            self.bpm_state = if byte == 0x1B { 1 } else { 0 };
        }

        // Intercept \033[6n (ANSI cursor-position query) in the TX stream before
        // it reaches the host terminal.  When the 4-byte sequence is complete:
        //   - suppress all 4 bytes from stdout
        //   - inject \033[1;1R into the guest RX buffer
        // This prevents the host from generating a real CPR response on stdin
        // which would race with our synthetic one and confuse readline.
        // Works on every occurrence, not just the first, so repeated shell
        // prompts never produce stray ^[[...R in the terminal.
        match (self.dsr_state, byte) {
            (_, 0x1B) => {
                self.flush_prefix();   // flush any prior partial match
                self.dsr_state = 1;    // hold ESC, wait for [
            }
            (1, b'[') => { self.dsr_state = 2; }
            (2, b'6') => { self.dsr_state = 3; }
            (3, b'n') => {
                for &b in b"\x1b[1;1R" { self.rx_buf.push_back(b); }
                self.stdin_ready = true;
                self.dsr_state = 0;
                // Sequence fully suppressed — do not print.
            }
            _ => {
                // Not \033[6n — flush any partial prefix, then output this byte.
                // Don't force-flush stdout here: Rust's LineWriter already flushes
                // on '\n' (which kernel boot output has plenty of), and the main
                // loop calls flush_pending() periodically for the prompt case.
                self.flush_prefix();
                print!("{}", byte as char);
                self.tx_bytes = self.tx_bytes.wrapping_add(1);
            }
        }
    }
}
