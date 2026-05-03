pub struct Clint {
    pub mtime: u64,
    pub mtimecmp: u64,
}

impl Clint {
    pub fn new() -> Self {
        Self { mtime: 0, mtimecmp: u64::MAX }
    }

    pub fn tick(&mut self) -> bool {
        self.mtime = self.mtime.wrapping_add(1);
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
        assert!(!c.tick()); // mtime=1, 1 < MAX → false
        assert!(!c.tick()); // mtime=2 → false
    }

    #[test]
    fn tick_returns_true_when_mtime_meets_mtimecmp() {
        let mut c = Clint::new();
        c.mtime    = 9;
        c.mtimecmp = 10;
        assert!(c.tick()); // mtime becomes 10, 10 >= 10 → true
    }

    #[test]
    fn tick_continues_true_while_mtime_exceeds_mtimecmp() {
        let mut c = Clint::new();
        c.mtime    = 10;
        c.mtimecmp = 10;
        assert!(c.tick()); // mtime=11, 11 >= 10 → true
    }
}
