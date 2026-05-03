pub struct Csr {
    pub mstatus:  u64,
    pub misa:     u64,
    pub mie:      u64,
    pub mtvec:    u64,
    pub mscratch: u64,
    pub mepc:     u64,
    pub mcause:   u64,
    pub mtval:    u64,
    pub mip:      u64,
    pub stvec:    u64,
    pub sscratch: u64,
    pub sepc:     u64,
    pub scause:   u64,
    pub stval:    u64,
    pub satp:     u64,
}

impl Csr {
    pub fn new() -> Self {
        Self {
            mstatus:  0,
            misa:     0x8000_0000_0000_1101,
            mie:      0,
            mtvec:    0,
            mscratch: 0,
            mepc:     0,
            mcause:   0,
            mtval:    0,
            mip:      0,
            stvec:    0,
            sscratch: 0,
            sepc:     0,
            scause:   0,
            stval:    0,
            satp:     0,
        }
    }

    pub fn read(&self, addr: u16) -> u64 {
        match addr {
            0x300 => self.mstatus,
            0x301 => self.misa,
            0x304 => self.mie,
            0x305 => self.mtvec,
            0x340 => self.mscratch,
            0x341 => self.mepc,
            0x342 => self.mcause,
            0x343 => self.mtval,
            0x344 => self.mip,
            0x105 => self.stvec,
            0x140 => self.sscratch,
            0x141 => self.sepc,
            0x142 => self.scause,
            0x143 => self.stval,
            0x180 => self.satp,
            // Read-only: hardwired zero
            0xf11 => 0, // mvendorid
            0xf12 => 0, // marchid
            0xf13 => 0, // mimpid
            0xf14 => 0, // mhartid
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, val: u64) {
        match addr {
            0x300 => self.mstatus  = val,
            0x301 => {}            // misa: read-only
            0x304 => self.mie      = val,
            0x305 => self.mtvec    = val,
            0x340 => self.mscratch = val,
            0x341 => self.mepc     = val,
            0x342 => self.mcause   = val,
            0x343 => self.mtval    = val,
            0x344 => self.mip      = val,
            0x105 => self.stvec    = val,
            0x140 => self.sscratch = val,
            0x141 => self.sepc     = val,
            0x142 => self.scause   = val,
            0x143 => self.stval    = val,
            0x180 => self.satp     = val,
            // Read-only: silently ignore
            0xf11..=0xf14 => {}
            _ => {} // unimplemented or read-only: silently ignore (Priv §2.1)
        }
    }
}

impl Default for Csr {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misa_hardwired() {
        let mut csr = Csr::new();
        assert_eq!(csr.read(0x301), 0x8000_0000_0000_1101);
        csr.write(0x301, 0);
        assert_eq!(csr.read(0x301), 0x8000_0000_0000_1101); // write ignored
    }

    #[test]
    fn mhartid_hardwired_zero() {
        let mut csr = Csr::new();
        assert_eq!(csr.read(0xf14), 0);
        csr.write(0xf14, 0xdead);
        assert_eq!(csr.read(0xf14), 0); // write ignored
    }

    #[test]
    fn mtvec_round_trips() {
        let mut csr = Csr::new();
        csr.write(0x305, 0x8000_1000);
        assert_eq!(csr.read(0x305), 0x8000_1000);
    }

    #[test]
    fn unknown_csr_reads_zero() {
        let csr = Csr::new();
        assert_eq!(csr.read(0x999), 0);
    }
}
