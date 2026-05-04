pub struct Clint {
    pub mtime: u64,
    pub mtimecmp: u64,
}

// mtime ticks advanced per call to tick().
// tick() is called every 1024 guest-tick batch (~128 JIT blocks, ~35K times/sec at
// JIT speed). The DTB advertises timebase-frequency=10_000_000 (10 MHz); Linux sets
// mtimecmp += 10_000_000/HZ ≈ 40_000 per scheduler tick (HZ=250).
// With INC=1: mtime advances 35K/s → 40K mtime per tick takes ~1.1s real time.
// With INC=128: mtime advances 4.48M/s → 40K mtime per tick takes ~9ms real time.
const MTIME_INC: u64 = 128;

impl Clint {
    pub fn new() -> Self {
        Self { mtime: 0, mtimecmp: u64::MAX }
    }

    pub fn tick(&mut self) -> bool {
        self.mtime = self.mtime.wrapping_add(MTIME_INC);
        self.mtime >= self.mtimecmp
    }

    pub fn load(&self, addr: u64, _width: usize) -> u64 {
        match addr {
            0x0200_BFF8 => self.mtime,
            0x0200_4000 => self.mtimecmp,
            _ => 0,
        }
    }

    pub fn store(&mut self, addr: u64, _width: usize, val: u64) {
        if addr == 0x0200_4000 {
            self.mtimecmp = val;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_returns_false_when_mtime_below_mtimecmp() {
        let mut c = Clint::new(); // mtime=0, mtimecmp=u64::MAX
        assert!(!c.tick()); // mtime=128, still < MAX → false
        assert!(!c.tick()); // mtime=256 → false
    }

    #[test]
    fn tick_returns_true_when_mtime_meets_mtimecmp() {
        let mut c = Clint::new();
        c.mtime    = 9;
        c.mtimecmp = 10;
        assert!(c.tick()); // mtime=137, 137 >= 10 → true
    }

    #[test]
    fn tick_continues_true_while_mtime_exceeds_mtimecmp() {
        let mut c = Clint::new();
        c.mtime    = 10;
        c.mtimecmp = 10;
        assert!(c.tick()); // mtime=138, 138 >= 10 → true
    }
}
