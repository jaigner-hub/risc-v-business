# RISC-V Emulator Phase 3 Design: S-mode Privilege + Sv39 MMU

## Goal

Pass all `rv64si-p-*` and `rv64ui-v-*` riscv-tests, advancing the emulator from
M-mode-only RV64IMA to a machine that supports S-mode supervisor privilege and
Sv39 virtual memory.

## Done criterion

All `rv64si-p-*` tests pass (S-mode privilege), AND all `rv64ui-v-*` tests pass
(basic RV64I instructions running under Sv39 virtual memory).

## Architecture

Two new capabilities, sequential: S-mode privilege first (unblocks `rv64si-p-*`),
then Sv39 MMU (unblocks `rv64ui-v-*`).

### New file

- `src/cpu/mmu.rs` — `Mmu` struct with 64-entry direct-mapped TLB, `translate()`,
  `flush()`; also defines `AccessType` and `MmuFault`

### Modified files

- `src/cpu/mod.rs` — `mode: PrivMode` and `mmu: Mmu` added to `Cpu`;
  `deliver_trap()` updated for delegation; `step()` fetches through MMU
- `src/cpu/csr.rs` — `medeleg`/`mideleg` storage; `sstatus`/`sie`/`sip` as filtered
  views; `s_trap_entry()`/`s_ret()` helpers
- `src/cpu/execute.rs` — all 8 load/store ops through `mmu.translate()`; SRET
  restores privilege from SPP; ECALL picks `mcause` by mode
- `src/cpu/decode.rs` — `SfenceVma` instruction variant added
- `build.rs` — `rv64si-p-*` and `rv64ui-v-*` patterns added
- `scripts/fetch-riscv-tests.sh` — those two suites added to copy step

---

## Section 1: S-mode Privilege

### 1.1 PrivMode

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivMode { U = 0, S = 1, M = 3 }
```

Added to `src/cpu/mod.rs`. `Cpu` gains `pub mode: PrivMode`, initialized to `M`.

### 1.2 New CSRs

**Stored fields** added to `Csr`:
- `medeleg: u64` (addr 0x302) — machine exception delegation register
- `mideleg: u64` (addr 0x303) — machine interrupt delegation register

**Filtered views** (no new storage — read/write through existing fields):

| CSR | Addr | Implementation |
|-----|------|----------------|
| sstatus | 0x100 | `mstatus & SSTATUS_MASK` on read; merge with mask on write |
| sie | 0x104 | `mie & 0x222` on read/write |
| sip | 0x144 | `mip & 0x222` on read/write |

`SSTATUS_MASK = 0x8000_0003_000D_E162` — the bits of mstatus visible to S-mode:
SD[63], UXL[33:32] (hardwired 2), MXR[19], SUM[18], XS[16:15], FS[14:13], SPP[8],
UBE[6], SPIE[5], SIE[1]. sstatus read = `(mstatus | 0x0000_000A_0000_0000) & SSTATUS_MASK`
(reuse the mstatus read path which already ORs in UXL/SXL, then mask to S-mode bits).

`scounteren` (0x106): stub — reads 0, writes ignored. Sufficient for Phase 3 tests.

### 1.3 S-mode trap helpers in `csr.rs`

```rust
pub fn s_trap_entry(&mut self, from_smode: bool) {
    let sie = (self.mstatus >> 1) & 1;
    self.mstatus = (self.mstatus & !0x122u64)   // clear SIE[1], SPIE[5], SPP[8]
        | (sie << 5)                             // SPIE ← SIE
        | ((from_smode as u64) << 8);            // SPP ← 1 if from S, 0 if from U
}

pub fn s_ret(&mut self) {
    let spie = (self.mstatus >> 5) & 1;
    self.mstatus = (self.mstatus & !0x122u64)   // clear SIE[1], SPIE[5], SPP[8]
        | (spie << 1)                            // SIE ← SPIE
        | (1u64 << 5);                           // SPIE ← 1, SPP ← 0 (U)
}
```

### 1.4 Trap delegation in `deliver_trap()`

```
if cause < 64 && (csr.medeleg >> cause) & 1 == 1 && mode != M:
    csr.s_trap_entry(mode == S)
    csr.sepc   = pc
    csr.scause = cause
    csr.stval  = tval
    pc = csr.stvec & !0b11
    mode = S
else:
    // existing M-mode path (unchanged)
```

Interrupt delegation (mideleg) is not needed for Phase 3 tests — only exception
delegation via medeleg is required.

### 1.5 SRET — full privilege restore

Current behavior: jump to sepc when TSR=0. Full behavior:

1. If `mstatus.TSR=1` → illegal instruction trap (already implemented, unchanged)
2. Else:
   - `mode ← if mstatus.SPP=1 then S else U`
   - Call `csr.s_ret()` (SIE←SPIE, SPIE←1, SPP←0)
   - `pc ← csr.sepc`

### 1.6 ECALL — mode-aware cause

| Mode | mcause |
|------|--------|
| U | 8 |
| S | 9 |
| M | 11 |

Delegation applies: if `(medeleg >> cause) & 1 == 1` and mode != M, the ECALL
delivers to S-mode via the delegation path in `deliver_trap()`.

---

## Section 2: Sv39 MMU

### 2.1 `src/cpu/mmu.rs`

```rust
const TLB_SIZE: usize = 64;

pub enum AccessType { Fetch, Load, Store }

pub struct MmuFault { pub cause: u64, pub tval: u64 }

struct TlbEntry {
    valid: bool,
    vpn:  u64,   // VA >> 12 (full 27-bit VPN for Sv39)
    ppn:  u64,   // physical page number
    perm: u8,    // PTE bits [7:0]: D/A/G/U/X/W/R/V
    asid: u16,
}

pub struct Mmu {
    tlb: [TlbEntry; TLB_SIZE],
}
```

### 2.2 `translate()` signature

```rust
pub fn translate(
    &mut self,
    bus:    &mut Bus,
    satp:   u64,
    mode:   PrivMode,
    mstatus: u64,
    addr:   u64,
    access: AccessType,
) -> Result<u64, MmuFault>
```

- If `satp >> 60 != 8` or `mode == M`: return `Ok(addr)` (physical passthrough)
- TLB lookup first; on hit, check permissions and return PA
- On miss: 3-level page table walk, fill TLB, return PA

### 2.3 Page table walk (Sv39)

VA layout: bits[63:39] must be sign-extension of bit[38] (else page fault);
VPN[2]=VA[38:30], VPN[1]=VA[29:21], VPN[0]=VA[20:12]; page offset=VA[11:0].

satp layout: MODE[63:60]=8, ASID[59:44], PPN[43:0].

```
pa = satp.PPN << 12
for level in [2, 1, 0]:
    pte_addr = pa + VPN[level] * 8
    pte = bus.load(pte_addr, 8)   // physical access; bus error → access fault
    if !pte.V or (pte.W && !pte.R): page fault
    if pte.R || pte.X:             // leaf PTE
        if level > 0:              // superpage
            if pte.PPN[level-1:0] != 0: page fault (misaligned superpage)
        check permissions (see §2.4)
        update A/D bits (see §2.5)
        fill TLB entry
        return ppn << 12 | offset  // offset includes lower VPN bits for superpages
    pa = pte.PPN << 12             // non-leaf: descend
page fault (no leaf found)
```

### 2.4 Permission checks

Performed on the leaf PTE after the walk:

| Access | Required | Mode constraint |
|--------|----------|-----------------|
| Fetch  | X=1      | U-page (U=1): only if mode=U |
| Load   | R=1, or X=1 when mstatus.MXR=1 | U-page: mode=U, or mode=S with mstatus.SUM=1 |
| Store  | W=1      | same as Load |

If mode=U and PTE.U=0: page fault.
If mode=S and PTE.U=1 and mstatus.SUM=0: page fault.

### 2.5 Hardware A/D bit updates

On every successful translation:
- If PTE.A=0: set PTE.A=1 in memory (write back to `pte_addr`) and in TLB entry
- On Store: if PTE.D=0: set PTE.D=1 in memory and in TLB entry

This avoids needing software A/D fault handlers and is required to pass
`rv64si-p-dirty`.

### 2.6 Fault causes

| Fault | cause | tval |
|-------|-------|------|
| Instruction page fault | 12 | faulting VA |
| Load page fault | 13 | faulting VA |
| Store/AMO page fault | 15 | faulting VA |

Physical bus errors after successful translation remain access faults:
- Fetch bus error: cause=1
- Load bus error: cause=5
- Store bus error: cause=7

### 2.7 TLB lookup

Index = `vpn & 63`. Hit condition:
```
entry.valid
  && entry.vpn == vpn
  && (entry.asid == satp.asid || entry.perm & G_BIT != 0)
```

On permission failure after a TLB hit: page fault (don't re-walk).

### 2.8 `flush()`

Invalidates all TLB entries (sets `valid = false`). Called by `SfenceVma`.
rs1/rs2 operands ignored — full flush only for Phase 3.

### 2.9 Integration in `step()`

```rust
// Instruction fetch — through MMU
let raw = match self.mmu.translate(&mut self.bus, self.csr.satp, self.mode,
                                    self.csr.mstatus, pc, AccessType::Fetch) {
    Ok(pa) => match self.bus.load(pa, 4) {
        Ok(v) => v as u32,
        Err(_) => { self.deliver_trap(1, pc); return Ok(()); }
    },
    Err(f) => { self.deliver_trap(f.cause, f.tval); return Ok(()); }
};
```

All 8 load/store instructions in `execute.rs` follow the same pattern:
translate → on fault, `deliver_trap(f.cause, f.tval)` and skip the register write.

### 2.10 `SfenceVma` in `decode.rs` / `execute.rs`

Encoding: opcode=0x73, funct3=0, funct7=0x09 (bits[31:25]). rs1 is the address
hint (bits[19:15]); rs2 is the ASID hint (bits[24:20]). The 12-bit `csr` field
extracted by the decoder = `(funct7 << 5) | rs2` = `0x120 | rs2`, ranging
0x120–0x13F depending on rs2 — so it cannot be matched as a fixed constant.

Decode arm: a guard under `funct3=0` before the `_` wildcard:
```rust
(csr, _, _) if (csr >> 5) == 0x09 => Ok(Instruction::SfenceVma),
```
rs1/rs2 operands not stored in the variant — full flush makes them irrelevant for
Phase 3.

Execute arm: `cpu.mmu.flush(); // SFENCE.VMA — full TLB flush`.

---

## Section 3: Test Infrastructure

### 3.1 `build.rs`

Add to the `is_test_elf` predicate:
```rust
|| name.starts_with("rv64si-p-")
|| name.starts_with("rv64ui-v-")
```

### 3.2 `scripts/fetch-riscv-tests.sh`

Add after the `rv64mi` copy block:
```bash
echo "Copying rv64si-p-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64si-p-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;

echo "Copying rv64ui-v-* to $DEST..."
find "$WORK/riscv-tests/isa" -name 'rv64ui-v-*' ! -name '*.dump' ! -name '*.o' \
    -exec cp {} "$DEST/" \;
```

Add summary lines:
```bash
echo "  rv64si: $(ls "$DEST" | grep -c 'rv64si-p-') ELFs"
echo "  rv64ui-v: $(ls "$DEST" | grep -c 'rv64ui-v-') ELFs"
```

### 3.3 Vendoring

Run `bash scripts/fetch-riscv-tests.sh` and `git add tests/riscv-tests/` as a
dedicated task before the test gate tasks. The `rv64ui-v-*` ELFs require the
`vm.h` virtual-memory test environment, which is part of the riscv-tests repo
and built by the existing `make isa` step.

### 3.4 Step limit

The current 100M step limit in `build.rs` is sufficient. Phase 2 tests all
completed well under 1M steps; `rv64ui-v-*` add a page-table setup preamble
but remain well within the limit.

---

## Spec references

- Privileged Spec v20211203 §3.1.8 (medeleg/mideleg), §4.1 (supervisor CSRs),
  §4.1.1 (sstatus), §4.3 (Sv39 page tables), §4.3.2 (A/D bits), §10 (SFENCE.VMA)
- Unprivileged Spec v20191213 §9 (Zicsr), §3.3 (ECALL/EBREAK)
