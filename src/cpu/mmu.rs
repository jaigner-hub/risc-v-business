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
        // Determine walk depth from satp.MODE. Sv39=8 (3 levels), Sv57=10 (5 levels).
        let satp_mode = satp >> 60;
        let levels: usize = match satp_mode {
            8  => 3,
            10 => 5,
            _  => return Ok(addr), // bare / unrecognised mode
        };
        if mode == PrivMode::M {
            return Ok(addr);
        }

        let vpn = addr >> 12;
        let page_offset = addr & 0xFFF;
        let asid = ((satp >> 44) & 0xFFFF) as u16;

        // Canonical VA check: top bits must sign-extend the MSB of the top VPN field.
        // Sv39: bits[63:39] sign-extend bit[38]. Sv57: bits[63:57] sign-extend bit[56].
        // sign_bit = 12 + levels * 9 - 1. Priv §4.3.1 (Sv39), §4.6 (Sv57).
        let sign_bit = 12 + levels * 9 - 1;
        let top = (addr as i64) >> sign_bit;
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

        // Page table walk (Sv39: 3 levels, Sv57: 5 levels). Priv §4.3.2 / §4.6.
        // vpn_parts[i] = VPN[levels-1-i], i.e. vpn_parts[0] is the top-level index.
        let mut vpn_parts = [0u64; 5];
        for i in 0..levels {
            vpn_parts[i] = (addr >> (12 + (levels - 1 - i) * 9)) & 0x1FF;
        }
        let mut table_pa = (satp & 0x0FFF_FFFF_FFFF) << 12;

        for level in 0..levels {
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
                let rem = levels - 1 - level;
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
    fn sv57_passthrough_when_m_mode() {
        let mut mmu = Mmu::new();
        let mut b = bus();
        let satp_sv57 = (10u64 << 60) | 0x8_0000;
        let result = mmu.translate(&mut b, satp_sv57, PrivMode::M, 0, 0xDEAD_0000, AccessType::Load);
        assert_eq!(result.unwrap(), 0xDEAD_0000);
    }

    #[test]
    fn sv57_bad_va_canonical_check() {
        let mut b = bus();
        // Sv57 canonical: bits[63:57] must sign-extend bit[56].
        // VA with bit 56=0 but bit 57 set is non-canonical.
        let satp: u64 = 10u64 << 60;
        let mut mmu = Mmu::new();
        let bad_va: u64 = 0x0200_0000_0000_0000; // bit 57 set, bit 56 = 0
        let result = mmu.translate(&mut b, satp, PrivMode::S, 0, bad_va, AccessType::Load);
        assert_eq!(result.unwrap_err().cause, 13); // load page fault
    }

    #[test]
    fn sv57_5level_page_walk() {
        // VA=0x0000_0000_0000_1ABC: all VPN fields 0 except VPN[0]=1.
        // 5 page tables × 4KB = 20KB; needs a 32KB bus.
        let mut b = Bus::new(0x8000, 0x8000_0000);
        // Level 4 table at 0x8000_0000. VPN[4]=0 → entry 0.
        let l3_ppn: u64 = 0x8000_1000 >> 12;
        b.store(0x8000_0000, 8, (l3_ppn << 10) | PTE_V).unwrap();
        // Level 3 table at 0x8000_1000. VPN[3]=0 → entry 0.
        let l2_ppn: u64 = 0x8000_2000 >> 12;
        b.store(0x8000_1000, 8, (l2_ppn << 10) | PTE_V).unwrap();
        // Level 2 table at 0x8000_2000. VPN[2]=0 → entry 0.
        let l1_ppn: u64 = 0x8000_3000 >> 12;
        b.store(0x8000_2000, 8, (l1_ppn << 10) | PTE_V).unwrap();
        // Level 1 table at 0x8000_3000. VPN[1]=0 → entry 0.
        let l0_ppn: u64 = 0x8000_4000 >> 12;
        b.store(0x8000_3000, 8, (l0_ppn << 10) | PTE_V).unwrap();
        // Level 0 (leaf) at 0x8000_4000. VPN[0] of 0x1ABC = 1 → PTE at offset 8.
        let target_ppn: u64 = 0x8001_0000 >> 12;
        let leaf_pte: u64 = (target_ppn << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D;
        b.store(0x8000_4000 + 1 * 8, 8, leaf_pte).unwrap();

        let satp: u64 = (10u64 << 60) | (0x8000_0000u64 >> 12);
        let mstatus: u64 = 1 << 18; // SUM=1
        let mut mmu = Mmu::new();
        let pa = mmu.translate(&mut b, satp, PrivMode::S, mstatus, 0x0000_0000_0000_1ABC, AccessType::Load).unwrap();
        assert_eq!(pa, 0x8001_0ABC);
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
