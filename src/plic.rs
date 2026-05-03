pub struct Plic;

impl Plic {
    pub fn new() -> Self {
        Self
    }

    pub fn load(&self, _addr: u64, _width: usize) -> u64 {
        0
    }

    pub fn store(&mut self, _addr: u64, _width: usize, _val: u64) {}
}
