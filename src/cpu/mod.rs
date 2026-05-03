pub mod decode;
pub mod execute;

use crate::bus::Bus;
use anyhow::Result;

pub struct Tracer {
    pub enabled: bool,
}

impl Tracer {
    pub fn new(enabled: bool) -> Self { Self { enabled } }

    pub fn trace_step(&self, pc: u64, raw: u32, mnemonic: &str, operands: &str,
                      reg_changes: &[(usize, u64, u64)]) {
        if !self.enabled { return; }
        let changes: String = reg_changes.iter()
            .map(|(r, old, new)| format!(" x{r}: {old:#018x} -> {new:#018x}"))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!("[{pc:#010x}] {raw:08x}  {mnemonic:<8} {operands:<24}{changes}");
    }
}

pub struct Cpu {
    regs:   [u64; 32],
    pub pc: u64,
    pub bus: Bus,
    pub tracer: Tracer,
}

impl Cpu {
    pub fn new(bus: Bus, entry: u64, trace: bool) -> Self {
        Self { regs: [0u64; 32], pc: entry, bus, tracer: Tracer::new(trace) }
    }

    /// Read register. x0 always returns 0.
    #[inline(always)]
    pub fn reg(&self, n: usize) -> u64 {
        assert!(n < 32, "register index out of range: {n}");
        if n == 0 { 0 } else { self.regs[n] }
    }

    /// Write register. Writes to x0 are silently ignored.
    #[inline(always)]
    pub fn set_reg(&mut self, n: usize, val: u64) {
        assert!(n < 32, "register index out of range: {n}");
        if n != 0 { self.regs[n] = val; }
    }

    /// Fetch, decode, execute one instruction. Advances pc.
    /// Fully wired up in Task 8 once decode() and execute() exist.
    pub fn step(&mut self) -> Result<()> {
        use decode::decode;
        use execute::execute;

        let raw = self.bus.load(self.pc, 4)? as u32;
        let inst = decode(raw)?;

        if self.tracer.enabled {
            let before = self.regs;
            let pc = self.pc;
            execute(self, inst)?;
            let changes: Vec<(usize, u64, u64)> = (1..32)
                .filter(|&i| before[i] != self.regs[i])
                .map(|i| (i, before[i], self.regs[i]))
                .collect();
            let inst2 = decode(raw).unwrap();
            let mnemonic = format!("{inst2:?}");
            let short = mnemonic.split(' ').next().unwrap_or(&mnemonic);
            self.tracer.trace_step(pc, raw, short, "", &changes);
        } else {
            execute(self, inst)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;

    fn cpu() -> Cpu {
        Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false)
    }

    #[test] fn x0_always_zero() {
        let mut c = cpu();
        c.set_reg(0, 0xDEAD);
        assert_eq!(c.reg(0), 0);
    }

    #[test] fn reg_read_write() {
        let mut c = cpu();
        c.set_reg(5, 0xCAFE);
        assert_eq!(c.reg(5), 0xCAFE);
    }
}
