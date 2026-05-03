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
        bus:     &mut Bus,
        satp:    u64,
        mode:    PrivMode,
        mstatus: u64,
        addr:    u64,
        access:  AccessType,
    ) -> Result<u64, MmuFault> {
        if (satp >> 60) != 8 || mode == PrivMode::M {
            return Ok(addr);
        }

        let vpn = addr >> 12;
        let page_offset = addr & 0xFFF;
        let asid = ((satp >> 44) & 0xFFFF) as u16;

        // Canonical VA check: bits[63:39] must sign-extend bit[38]. Priv §4.3.1
        let top = (addr as i64) >> 38;
        if top != 0 && top != -1 {
            return Err(MmuFault { cause: pf_cause(access), tval: addr });
        }

        // TLB lookup: direct-mapped by vpn & (TLB_SIZE-1)
        let idx = (vpn & (TLB_SIZE as u64 - 1)) as usize;
        {
            let e = self.tlb[idx];
            if e.valid && e.vpn == vpn && (e.asid == asid || e.perm & PTE_G != 0) {
                check_perms(e.perm, mstatus, mode, access, addr)?;
                // Hardware A/D update on TLB hit
                let mut new_perm = e.perm | PTE_A;
                if access == AccessType::Store { new_perm |= PTE_D; }
                if new_perm != e.perm {
                    let new_pte = (e.ppn << 10) | new_perm;
                    bus.store(e.pte_addr, 8, new_pte).ok();
                    self.tlb[idx].perm = new_perm;
                }
                return Ok((e.ppn << 12) | page_offset);
            }
        }

        // 3-level Sv39 page table walk. Priv §4.3.2.
        // VA layout: VPN[2]=bits[38:30], VPN[1]=bits[29:21], VPN[0]=bits[20:12]
        let vpn_parts = [(addr >> 30) & 0x1FF, (addr >> 21) & 0x1FF, (addr >> 12) & 0x1FF];
        let mut table_pa = (satp & 0x0FFF_FFFF_FFFF) << 12;

        for level in 0usize..3 {
            let pte_addr = table_pa + vpn_parts[level] * 8;
            let pte = bus.load(pte_addr, 8)
                .map_err(|_| MmuFault { cause: af_cause(access), tval: addr })?;

            if pte & PTE_V == 0 || (pte & PTE_W != 0 && pte & PTE_R == 0) {
                return Err(MmuFault { cause: pf_cause(access), tval: addr });
            }

            if pte & (PTE_R | PTE_X) != 0 {
                // Leaf PTE found at this level
                let ppn = (pte >> 10) & 0x0FFF_FFFF_FFFF;

                // Superpage alignment check: lower VPN bits of PPN must be zero. Priv §4.3.2 step 5.
                let rem = 2 - level; // 0=4K, 1=2MB, 2=1GB
                if rem > 0 && ppn & ((1u64 << (rem * 9)) - 1) != 0 {
                    return Err(MmuFault { cause: pf_cause(access), tval: addr });
                }

                check_perms(pte & 0xFF, mstatus, mode, access, addr)?;

                // Hardware A/D bit update. Priv §4.3.2 step 7.
                let mut new_pte = pte | PTE_A;
                if access == AccessType::Store { new_pte |= PTE_D; }
                if new_pte != pte {
                    bus.store(pte_addr, 8, new_pte)
                        .map_err(|_| MmuFault { cause: af_cause(access), tval: addr })?;
                }

                // Compute PA (superpage-aware)
                let page_bits = 12 + rem * 9;
                let page_size = 1u64 << page_bits;
                let pa = ((ppn >> (rem * 9)) << page_bits) | (addr & (page_size - 1));

                // Fill TLB for 4K pages only (rem == 0)
                if rem == 0 {
                    self.tlb[idx] = TlbEntry {
                        valid: true, vpn, ppn, asid,
                        perm: new_pte & 0xFF,
                        pte_addr,
                    };
                }

                return Ok(pa);
            }

            // Non-leaf: descend to next level
            table_pa = ((pte >> 10) & 0x0FFF_FFFF_FFFF) << 12;
        }

        Err(MmuFault { cause: pf_cause(access), tval: addr })
    }
}

fn pf_cause(access: AccessType) -> u64 {
    match access { AccessType::Fetch => 12, AccessType::Load => 13, AccessType::Store => 15 }
}

fn af_cause(access: AccessType) -> u64 {
    match access { AccessType::Fetch => 1, AccessType::Load => 5, AccessType::Store => 7 }
}

fn check_perms(perm: u64, mstatus: u64, mode: PrivMode, access: AccessType, tval: u64) -> Result<(), MmuFault> {
    let fault = MmuFault { cause: pf_cause(access), tval };
    let u_bit = perm & PTE_U != 0;
    match mode {
        PrivMode::U => { if !u_bit { return Err(fault); } }
        PrivMode::S => {
            // S-mode cannot access U-pages unless mstatus.SUM=1. Priv §4.3.1.
            if u_bit && (mstatus >> 18) & 1 == 0 { return Err(fault); }
        }
        PrivMode::M => {}
    }
    match access {
        AccessType::Fetch => { if perm & PTE_X == 0 { return Err(fault); } }
        AccessType::Load  => {
            // MXR: execute-only pages are readable. Priv §4.1.1
            let mxr = (mstatus >> 19) & 1;
            if perm & PTE_R == 0 && !(mxr != 0 && perm & PTE_X != 0) { return Err(fault); }
        }
        AccessType::Store => { if perm & PTE_W == 0 { return Err(fault); } }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::cpu::PrivMode;

    fn bus() -> Bus { Bus::new(0x4000, 0x8000_0000) }

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

    #[test]
    fn sv39_4k_page_walk() {
        // VA 0x0000_1ABC → PA 0x8001_0ABC (4K page, R/W/X/U, A/D pre-set)
        let mut b = bus();

        // Root table at 0x8000_0000. VPN[2] of 0x0000_1000 = 0.
        let l2_ppn: u64 = 0x8000_1000 >> 12;
        b.store(0x8000_0000, 8, (l2_ppn << 10) | PTE_V).unwrap();

        // Level-1 table at 0x8000_1000. VPN[1] of 0x0000_1000 = 0.
        let l1_ppn: u64 = 0x8000_2000 >> 12;
        b.store(0x8000_1000, 8, (l1_ppn << 10) | PTE_V).unwrap();

        // Leaf at 0x8000_2000. VPN[0] of 0x0000_1000 = 1 → PTE at offset 8.
        let target_ppn: u64 = 0x8001_0000 >> 12;
        let leaf_pte: u64 = (target_ppn << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D;
        b.store(0x8000_2000 + 1 * 8, 8, leaf_pte).unwrap();

        let satp: u64 = (8u64 << 60) | (0x8000_0000 >> 12);
        let mstatus: u64 = 1 << 18; // SUM=1 so S-mode may access U-mapped page
        let mut mmu = Mmu::new();
        let pa = mmu.translate(&mut b, satp, PrivMode::S, mstatus, 0x0000_1ABC, AccessType::Load).unwrap();
        assert_eq!(pa, 0x8001_0ABC);
    }

    #[test]
    fn sv39_bad_va_canonical_check() {
        let mut b = bus();
        let satp: u64 = 8u64 << 60;
        let mut mmu = Mmu::new();
        let bad_va: u64 = 0x0080_0000_0000; // bit 38=0 but bits 39+ set → non-canonical
        let result = mmu.translate(&mut b, satp, PrivMode::S, 0, bad_va, AccessType::Load);
        assert_eq!(result.unwrap_err().cause, 13); // load page fault
    }

    #[test]
    fn sv39_tlb_hit_after_first_walk() {
        let mut b = bus();
        let l2_ppn: u64 = 0x8000_1000 >> 12;
        b.store(0x8000_0000, 8, (l2_ppn << 10) | PTE_V).unwrap();
        let l1_ppn: u64 = 0x8000_2000 >> 12;
        b.store(0x8000_1000, 8, (l1_ppn << 10) | PTE_V).unwrap();
        let target_ppn: u64 = 0x8001_0000 >> 12;
        let leaf_pte: u64 = (target_ppn << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D;
        b.store(0x8000_2000 + 1 * 8, 8, leaf_pte).unwrap();

        let satp: u64 = (8u64 << 60) | (0x8000_0000 >> 12);
        let mstatus: u64 = 1 << 18; // SUM=1 so S-mode may access U-mapped page
        let mut mmu = Mmu::new();
        let pa1 = mmu.translate(&mut b, satp, PrivMode::S, mstatus, 0x0000_1000, AccessType::Load).unwrap();
        let pa2 = mmu.translate(&mut b, satp, PrivMode::S, mstatus, 0x0000_1100, AccessType::Load).unwrap();
        assert_eq!(pa1, 0x8001_0000);
        assert_eq!(pa2, 0x8001_0100); // same page, different offset
    }
}
