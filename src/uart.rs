use std::io::{self, Write};

pub struct Uart16550;

impl Uart16550 {
    pub fn new() -> Self {
        Self
    }

    pub fn load(&self, addr: u64, _width: usize) -> u64 {
        match addr & 0xFF {
            5 => 0x60, // LSR: TX-empty + THR-empty (bits 5+6)
            _ => 0,
        }
    }

    pub fn store(&mut self, addr: u64, _width: usize, val: u64) {
        if addr & 0xFF == 0 {
            // THR write: emit the character
            print!("{}", val as u8 as char);
            let _ = io::stdout().flush();
        }
    }
}
