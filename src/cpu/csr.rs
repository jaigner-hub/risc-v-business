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
    pub pmpcfg0:  u64,
    pub pmpcfg2:  u64,
    pub pmpaddr:  [u64; 16],
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
            pmpcfg0:  0,
            pmpcfg2:  0,
            pmpaddr:  [0u64; 16],
        }
    }

    pub fn read(&self, addr: u16) -> u64 {
        match addr {
            // UXL[33:32]=2 and SXL[35:34]=2 are hardwired for RV64. Priv §3.1.6.2
            0x300 => self.mstatus | 0x0000_000A_0000_0000,
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
            0x3A0 => self.pmpcfg0,
            0x3A2 => self.pmpcfg2,
            0x3B0..=0x3BF => self.pmpaddr[(addr - 0x3B0) as usize],
            // Read-only: hardwired zero
            0xf11 => 0, // mvendorid
            0xf12 => 0, // marchid
            0xf13 => 0, // mimpid
            0xf14 => 0, // mhartid
            _ => 0,
        }
    }

    pub fn mie_bit(&self) -> u64 {
        (self.mstatus >> 3) & 1
    }

    pub fn trap_entry(&mut self) {
        let mie = self.mie_bit();
        self.mstatus = (self.mstatus & !0x1888u64) // clear MIE[3], MPIE[7], MPP[12:11]
            | (mie << 7)                           // MPIE ← MIE
            | (3u64 << 11);                        // MPP ← M-mode
    }

    pub fn mret(&mut self) {
        let mpie = (self.mstatus >> 7) & 1;
        self.mstatus = (self.mstatus & !0x1888u64) // clear MIE[3], MPIE[7], MPP[12:11]
            | (mpie << 3)                          // MIE ← MPIE
            | (1u64 << 7);                         // MPIE ← 1
    }

    pub fn write(&mut self, addr: u16, val: u64) {
        match addr {
            // UXL[33:32] and SXL[35:34] are read-only; mask them out on write
            0x300 => self.mstatus  = val & !0x0000_000F_0000_0000u64,
            0x301 => {}            // misa: read-only
            0x304 => self.mie      = val,
            0x305 => self.mtvec    = val & !0x3,  // only direct mode (MODE=0) supported
            0x340 => self.mscratch = val,
            0x341 => self.mepc     = val & !0x3,  // IALIGN=32: bits[1:0] always 0
            0x342 => self.mcause   = val,
            0x343 => self.mtval    = val,
            0x344 => self.mip      = val,
            0x105 => self.stvec    = val,
            0x140 => self.sscratch = val,
            0x141 => self.sepc     = val,
            0x142 => self.scause   = val,
            0x143 => self.stval    = val,
            0x180 => self.satp     = val,
            0x3A0 => self.pmpcfg0  = val,
            0x3A2 => self.pmpcfg2  = val,
            0x3B0..=0x3BF => self.pmpaddr[(addr - 0x3B0) as usize] = val,
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

    #[test]
    fn mstatus_mie_mpie_mpp() {
        let mut csr = Csr::new();
        // Set MIE=1 (bit 3)
        csr.mstatus = 0b1000; // MIE=1
        assert_eq!(csr.mie_bit(), 1);
        // Simulate trap entry: MPIE←MIE, MIE←0, MPP←3
        csr.trap_entry();
        assert_eq!(csr.mie_bit(), 0);           // MIE cleared
        assert_eq!((csr.mstatus >> 7) & 1, 1);  // MPIE = previous MIE
        assert_eq!((csr.mstatus >> 11) & 3, 3); // MPP = M-mode
        // Simulate MRET: MIE←MPIE, MPIE←1
        csr.mret();
        assert_eq!(csr.mie_bit(), 1);           // MIE restored
        assert_eq!((csr.mstatus >> 7) & 1, 1);  // MPIE set to 1
    }
}
