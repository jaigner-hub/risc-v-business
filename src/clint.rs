pub struct Clint {
    pub mtime: u64,
    pub mtimecmp: u64,
}

impl Clint {
    pub fn new() -> Self {
        Self { mtime: 0, mtimecmp: u64::MAX }
    }

    pub fn tick(&mut self) {
        self.mtime = self.mtime.wrapping_add(1);
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
