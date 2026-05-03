// PLIC base: 0x0C000000
// Memory map:
//   0x000000 + irq*4       : IRQ priority (irq 0..127)
//   0x001000               : pending bits [63:0] (read-only)
//   0x001004               : pending bits [127:64] (read-only)
//   0x002000 + ctx*0x80    : enable bits word 0 (IRQs 0-31) for context ctx
//   0x002004 + ctx*0x80    : enable bits word 1 (IRQs 32-63)
//   0x200000 + ctx*0x1000  : priority threshold for context ctx
//   0x200004 + ctx*0x1000  : claim (read) / complete (write) for context ctx
//
// Contexts: 0 = M-mode hart 0, 1 = S-mode hart 0

const BASE: u64 = 0x0C00_0000;

pub struct Plic {
    priority: [u32; 128],
    enable: [[u32; 4]; 2],
    threshold: [u32; 2],
    pending: u64,
}

impl Plic {
    pub fn new() -> Self {
        Self {
            priority: [0; 128],
            enable: [[0; 4]; 2],
            threshold: [0; 2],
            pending: 0,
        }
    }

    pub fn set_pending(&mut self, irq: u32, active: bool) {
        if irq < 64 {
            if active {
                self.pending |= 1u64 << irq;
            } else {
                self.pending &= !(1u64 << irq);
            }
        }
    }

    /// Returns true if S-mode context (1) has a claimable interrupt.
    pub fn has_interrupt(&self) -> bool {
        self.best_irq(1) != 0
    }

    fn best_irq(&self, ctx: usize) -> u32 {
        let mut best_irq = 0u32;
        let mut best_pri = self.threshold[ctx];
        for irq in 1u32..64 {
            if (self.pending >> irq) & 1 == 0 {
                continue;
            }
            let word = (irq / 32) as usize;
            let bit = irq % 32;
            if (self.enable[ctx][word] >> bit) & 1 == 0 {
                continue;
            }
            let pri = self.priority[irq as usize];
            if pri > best_pri {
                best_pri = pri;
                best_irq = irq;
            }
        }
        best_irq
    }

    pub fn load(&mut self, addr: u64, _width: usize) -> u64 {
        let off = addr - BASE;
        match off {
            // IRQ priority registers
            0x000000..=0x0001FF => {
                let irq = (off / 4) as usize;
                if irq < 128 { self.priority[irq] as u64 } else { 0 }
            }
            // Pending bits (read-only)
            0x001000..=0x00107F => {
                let word = ((off - 0x001000) / 4) as usize;
                match word {
                    0 => self.pending as u32 as u64,
                    1 => (self.pending >> 32) as u64,
                    _ => 0,
                }
            }
            // Enable registers
            0x002000..=0x003FFF => {
                let ctx = ((off - 0x002000) / 0x80) as usize;
                let word = (((off - 0x002000) % 0x80) / 4) as usize;
                if ctx < 2 && word < 4 { self.enable[ctx][word] as u64 } else { 0 }
            }
            // Per-context threshold and claim/complete
            0x200000..=0x20FFFF => {
                let ctx = ((off - 0x200000) / 0x1000) as usize;
                let local = (off - 0x200000) % 0x1000;
                if ctx >= 2 {
                    return 0;
                }
                match local {
                    0 => self.threshold[ctx] as u64,
                    4 => {
                        // Claim: atomically return best IRQ and clear its pending bit
                        let irq = self.best_irq(ctx);
                        if irq > 0 {
                            self.pending &= !(1u64 << irq);
                        }
                        irq as u64
                    }
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    pub fn store(&mut self, addr: u64, _width: usize, val: u64) {
        let off = addr - BASE;
        match off {
            // IRQ priority registers
            0x000000..=0x0001FF => {
                let irq = (off / 4) as usize;
                if irq < 128 {
                    self.priority[irq] = val as u32;
                }
            }
            // Enable registers
            0x002000..=0x003FFF => {
                let ctx = ((off - 0x002000) / 0x80) as usize;
                let word = (((off - 0x002000) % 0x80) / 4) as usize;
                if ctx < 2 && word < 4 {
                    self.enable[ctx][word] = val as u32;
                }
            }
            // Per-context threshold and complete
            0x200000..=0x20FFFF => {
                let ctx = ((off - 0x200000) / 0x1000) as usize;
                let local = (off - 0x200000) % 0x1000;
                if ctx >= 2 {
                    return;
                }
                match local {
                    0 => self.threshold[ctx] = val as u32,
                    4 => {
                        // Complete: clear pending for the acknowledged IRQ
                        let irq = val as u32;
                        if irq > 0 && irq < 64 {
                            self.pending &= !(1u64 << irq);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
