use anyhow::{anyhow, Result};
use crate::clint::Clint;
use crate::plic::Plic;
use crate::uart::Uart16550;
use crate::virtio_blk::VirtioBlk;

pub struct Bus {
    ram: Vec<u8>,
    pub ram_base: u64,
    pub clint: Clint,
    pub uart: Uart16550,
    pub plic: Plic,
    pub virtio_blk: VirtioBlk,
}

impl Bus {
    pub fn new(size: usize, ram_base: u64) -> Self {
        Self {
            ram: vec![0u8; size],
            ram_base,
            clint: Clint::new(),
            uart: Uart16550::new(),
            plic: Plic::new(),
            virtio_blk: VirtioBlk::new(None),
        }
    }

    pub fn ram_mut(&mut self) -> &mut Vec<u8> {
        &mut self.ram
    }

    /// Load `width` bytes (1/2/4/8) from `addr`, zero-extended to u64.
    pub fn load(&mut self, addr: u64, width: usize) -> Result<u64> {
        match addr {
            0x0200_0000..=0x0200_FFFF => Ok(self.clint.load(addr, width)),
            0x0C00_0000..=0x0FFF_FFFF => Ok(self.plic.load(addr, width)),
            0x1000_0000..=0x1000_00FF => Ok(self.uart.load(addr, width)),
            0x1000_1000..=0x1000_11FF => Ok(self.virtio_blk.load(addr - 0x1000_1000, width)),
            _ => {
                let off = self.offset(addr, width)?;
                Ok(match width {
                    1 => self.ram[off] as u64,
                    2 => u16::from_le_bytes(self.ram[off..off + 2].try_into().unwrap()) as u64,
                    4 => u32::from_le_bytes(self.ram[off..off + 4].try_into().unwrap()) as u64,
                    8 => u64::from_le_bytes(self.ram[off..off + 8].try_into().unwrap()),
                    _ => unreachable!("invalid load width {width}"),
                })
            }
        }
    }

    /// Store `width` bytes (1/2/4/8) of `value` to `addr`.
    pub fn store(&mut self, addr: u64, width: usize, value: u64) -> Result<()> {
        match addr {
            0x0200_0000..=0x0200_FFFF => { self.clint.store(addr, width, value); Ok(()) }
            0x0C00_0000..=0x0FFF_FFFF => { self.plic.store(addr, width, value); Ok(()) }
            0x1000_0000..=0x1000_00FF => { self.uart.store(addr, width, value); Ok(()) }
            0x1000_1000..=0x1000_11FF => {
                let notify = self.virtio_blk.store(addr - 0x1000_1000, width, value);
                if notify {
                    let base = self.ram_base;
                    self.virtio_blk.process_queue(&mut self.ram, base);
                }
                Ok(())
            }
            _ => {
                let off = self.offset(addr, width)?;
                match width {
                    1 => self.ram[off] = value as u8,
                    2 => self.ram[off..off + 2].copy_from_slice(&(value as u16).to_le_bytes()),
                    4 => self.ram[off..off + 4].copy_from_slice(&(value as u32).to_le_bytes()),
                    8 => self.ram[off..off + 8].copy_from_slice(&value.to_le_bytes()),
                    _ => unreachable!("invalid store width {width}"),
                }
                Ok(())
            }
        }
    }

    fn offset(&self, addr: u64, width: usize) -> Result<usize> {
        let end = addr
            .checked_add(width as u64)
            .ok_or_else(|| anyhow!("bus fault: addr={addr:#x} width={width}"))?;
        let ram_end = self.ram_base + self.ram.len() as u64;
        if addr < self.ram_base || end > ram_end {
            return Err(anyhow!("bus fault: addr={addr:#x} width={width}"));
        }
        Ok((addr - self.ram_base) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> Bus {
        Bus::new(64, 0x8000_0000)
    }

    #[test]
    fn load_store_u8() {
        let mut b = bus();
        b.store(0x8000_0000, 1, 0xAB).unwrap();
        assert_eq!(b.load(0x8000_0000, 1).unwrap(), 0xAB);
    }

    #[test]
    fn load_store_u16_le() {
        let mut b = bus();
        b.store(0x8000_0000, 2, 0x1234).unwrap();
        assert_eq!(b.load(0x8000_0000, 1).unwrap(), 0x34);
        assert_eq!(b.load(0x8000_0001, 1).unwrap(), 0x12);
    }

    #[test]
    fn load_store_u64() {
        let mut b = bus();
        b.store(0x8000_0000, 8, 0xDEADBEEF_CAFEBABE).unwrap();
        assert_eq!(b.load(0x8000_0000, 8).unwrap(), 0xDEADBEEF_CAFEBABE);
    }

    #[test]
    fn out_of_bounds_returns_err() {
        let mut b = bus();
        assert!(b.load(0x0000_0000, 4).is_err());
        assert!(b.load(0x8000_0040, 4).is_err());
    }

    #[test]
    fn clint_mtime_readable() {
        let mut b = bus();
        assert_eq!(b.load(0x0200_BFF8, 8).unwrap(), 0);
    }

    #[test]
    fn uart_lsr_returns_0x60() {
        let mut b = bus();
        assert_eq!(b.load(0x1000_0005, 1).unwrap(), 0x60);
    }

    #[test]
    fn plic_reads_zero() {
        let mut b = bus();
        assert_eq!(b.load(0x0C00_0000, 4).unwrap(), 0);
    }
}
