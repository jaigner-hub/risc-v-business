use crate::bus::Bus;
use super::PrivMode;

const TLB_SIZE: usize = 64;

pub(super) const PTE_V: u64 = 1 << 0;
pub(super) const PTE_R: u64 = 1 << 1;
pub(super) const PTE_W: u64 = 1 << 2;
pub(super) const PTE_X: u64 = 1 << 3;
pub(super) const PTE_U: u64 = 1 << 4;
pub(super) const PTE_G: u64 = 1 << 5;
pub(super) const PTE_A: u64 = 1 << 6;
pub(super) const PTE_D: u64 = 1 << 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType { Fetch, Load, Store }

#[derive(Debug)]
pub struct MmuFault { pub cause: u64, pub tval: u64 }

#[derive(Clone, Copy)]
pub(super) struct TlbEntry {
    pub valid:    bool,
    pub vpn:      u64,
    pub ppn:      u64,
    pub perm:     u64,
    pub asid:     u16,
    pub pte_addr: u64,
}

pub struct Mmu {
    pub(super) tlb: [TlbEntry; TLB_SIZE],
}

impl Mmu {
    pub fn new() -> Self {
        const EMPTY: TlbEntry = TlbEntry {
            valid: false, vpn: 0, ppn: 0, perm: 0, asid: 0, pte_addr: 0,
        };
        Self { tlb: [EMPTY; TLB_SIZE] }
    }

    pub fn flush(&mut self) {
        for e in &mut self.tlb { e.valid = false; }
    }

    pub fn translate(
        &mut self,
        _bus:     &mut Bus,
        satp:     u64,
        mode:     PrivMode,
        _mstatus: u64,
        addr:     u64,
        _access:  AccessType,
    ) -> Result<u64, MmuFault> {
        if (satp >> 60) != 8 || mode == PrivMode::M {
            return Ok(addr);
        }
        // Full Sv39 walk in Task 6
        Ok(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::cpu::PrivMode;

    fn bus() -> Bus { Bus::new(64, 0x8000_0000) }

    #[test]
    fn passthrough_when_m_mode() {
        let mut mmu = Mmu::new();
        let mut b = bus();
        let satp_sv39 = (8u64 << 60) | 0x8_0000;
        let result = mmu.translate(&mut b, satp_sv39, PrivMode::M, 0, 0xDEAD_0000, AccessType::Load);
        assert_eq!(result.unwrap(), 0xDEAD_0000);
    }

    #[test]
    fn passthrough_when_satp_mode_zero() {
        let mut mmu = Mmu::new();
        let mut b = bus();
        let satp_bare = 0u64;
        let result = mmu.translate(&mut b, satp_bare, PrivMode::S, 0, 0x8000_0000, AccessType::Load);
        assert_eq!(result.unwrap(), 0x8000_0000);
    }

    #[test]
    fn flush_invalidates_all_entries() {
        let mut mmu = Mmu::new();
        mmu.tlb[0].valid = true;
        mmu.tlb[5].valid = true;
        mmu.flush();
        assert!(mmu.tlb.iter().all(|e| !e.valid));
    }
}
