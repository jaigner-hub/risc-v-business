use anyhow::{anyhow, Context, Result};
use goblin::elf::{Elf, header::ET_DYN, program_header::PT_LOAD};
use crate::bus::Bus;

pub struct LoadedElf {
    pub bus:         Bus,
    pub entry:       u64,
    pub tohost_addr: Option<u64>,
}

const RAM_BASE: u64 = 0x8000_0000;
const RAM_SIZE: usize = 256 * 1024 * 1024; // 256 MiB

/// Load an ELF64 binary: copy PT_LOAD segments into RAM, return entry point
/// and (if present) the address of the `tohost` symbol used by riscv-tests.
pub fn load_elf(bytes: &[u8]) -> Result<LoadedElf> {
    let elf = Elf::parse(bytes).context("ELF parse failed")?;
    if !elf.is_64 {
        return Err(anyhow!("expected ELF64, got ELF32"));
    }

    // PIE (ET_DYN) ELFs have addresses relative to zero; load them at RAM_BASE.
    // ET_EXEC ELFs (riscv-tests) are linked with p_paddr >= RAM_BASE already.
    let is_pie = elf.header.e_type == ET_DYN;
    let load_offset: u64 = if is_pie { RAM_BASE } else { 0 };

    let mut bus = Bus::new(RAM_SIZE, RAM_BASE);

    for ph in &elf.program_headers {
        if ph.p_type != PT_LOAD { continue; }
        let file_start = ph.p_offset as usize;
        let file_end   = file_start + ph.p_filesz as usize;
        // p_paddr: physical-memory test ELFs (rv64ui-p-*) are linked flat;
        // MMU-enabled phases still load to physical addresses via Sv39.
        // For PIE ELFs, add load_offset to relocate from 0 to RAM_BASE.
        let mem_addr = ph.p_paddr + load_offset;

        if mem_addr < RAM_BASE {
            return Err(anyhow!("PT_LOAD segment at {mem_addr:#x} is below RAM base {RAM_BASE:#x}"));
        }

        let segment = bytes.get(file_start..file_end)
            .ok_or_else(|| anyhow!("PT_LOAD segment out of file bounds"))?;

        let off = (mem_addr - RAM_BASE) as usize;
        if off + segment.len() > RAM_SIZE {
            return Err(anyhow!(
                "PT_LOAD segment [{mem_addr:#x}..{:#x}) exceeds RAM",
                mem_addr + ph.p_filesz
            ));
        }
        bus.ram_mut()[off..off + segment.len()].copy_from_slice(segment);
    }

    let tohost_addr = elf.syms.iter()
        .find(|s| elf.strtab.get_at(s.st_name) == Some("tohost"))
        .map(|s| s.st_value);

    Ok(LoadedElf { bus, entry: elf.entry + load_offset, tohost_addr })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_size_constant_is_256_mib() {
        assert_eq!(RAM_SIZE, 256 * 1024 * 1024);
    }
}
