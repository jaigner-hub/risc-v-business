// virtio-blk MMIO device — virtio spec 1.1, Section 4.2 (modern transport)
//
// Memory map base: 0x1000_1000
// IRQ: 1 (via PLIC)
//
// Virtqueue layout (set by driver via QueueDesc/Driver/DeviceLow/High):
//   desc table  : QUEUE_SIZE × 16-byte descriptors
//   driver ring : flags(u16) idx(u16) ring[QUEUE_SIZE](u16) ...
//   device ring : flags(u16) idx(u16) ring[QUEUE_SIZE](used_elem{id,len}) ...
//
// Each virtio-blk request uses a 3-descriptor chain:
//   [0] header  (12 bytes, host-read):  { u32 type, u32 reserved, u64 sector }
//   [1] data    (N×512 bytes, device-write for READ / host-read for WRITE)
//   [2] status  (1 byte, device-write): 0=ok, 1=ioerr

use std::fs::File;
use std::io::{Read, Write, Seek, SeekFrom};

const VIRTIO_MAGIC: u32    = 0x7472_6976; // "virt" little-endian
const VIRTIO_VERSION: u32  = 2;           // modern (non-legacy)
const VIRTIO_DEVICE_BLK: u32 = 2;
const VIRTIO_VENDOR: u32   = 0x554D_4551; // "QEMU"

const QUEUE_SIZE: u16 = 256;
const SECTOR_SIZE: u64 = 512;

// Device feature words.  Word 1 bit 0 = VIRTIO_F_VERSION_1 (bit 32 overall).
const DEVICE_FEATURES: [u32; 2] = [0, 1];

// Descriptor flags
const DESC_F_NEXT:  u16 = 1;
const DESC_F_WRITE: u16 = 2;

// Request types
const T_IN:    u32 = 0; // read from disk
const T_OUT:   u32 = 1; // write to disk
const T_FLUSH: u32 = 4; // flush — treated as NOP (synchronous I/O)

// Status bytes written into the last descriptor
const S_OK:    u8 = 0;
const S_IOERR: u8 = 1;

pub struct VirtioBlk {
    disk: Option<File>,
    capacity: u64,          // total sectors (512 bytes each)
    pub debug: bool,

    // MMIO register shadows
    device_features_sel: u32,
    driver_features: [u32; 2],
    driver_features_sel: u32,
    pub device_status: u32,
    queue_num: u32,
    queue_ready: u32,
    desc_addr:  u64,        // guest PA of descriptor table
    avail_addr: u64,        // guest PA of driver (available) ring
    used_addr:  u64,        // guest PA of device (used) ring
    pub irq_status: u32,    // bit 0 = used-buffer notification pending
    last_avail_idx: u16,    // available ring index processed so far
}

impl VirtioBlk {
    pub fn new(disk: Option<File>) -> Self {
        let capacity = disk
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map(|m| m.len() / SECTOR_SIZE)
            .unwrap_or(0);
        Self {
            disk,
            capacity,
            debug: false,
            device_features_sel: 0,
            driver_features: [0; 2],
            driver_features_sel: 0,
            device_status: 0,
            queue_num: QUEUE_SIZE as u32,
            queue_ready: 0,
            desc_addr: 0,
            avail_addr: 0,
            used_addr: 0,
            irq_status: 0,
            last_avail_idx: 0,
        }
    }

    pub fn load(&self, off: u64, _width: usize) -> u64 {
        let v = match off {
            0x000 => VIRTIO_MAGIC as u64,
            0x004 => VIRTIO_VERSION as u64,
            0x008 => VIRTIO_DEVICE_BLK as u64,
            0x00C => VIRTIO_VENDOR as u64,
            0x010 => DEVICE_FEATURES[self.device_features_sel.min(1) as usize] as u64,
            0x034 => QUEUE_SIZE as u64,     // QueueNumMax
            0x038 => self.queue_num as u64,
            0x044 => self.queue_ready as u64,
            0x060 => self.irq_status as u64,
            0x070 => self.device_status as u64,
            0x0FC => 0,  // ConfigGeneration (static config, always 0)
            // Block device config: capacity as two u32 words (little-endian u64)
            0x100 => self.capacity as u32 as u64,
            0x104 => (self.capacity >> 32) as u64,
            _ => 0,
        };
        if self.debug {
            let name = match off {
                0x000 => "MagicValue",  0x004 => "Version",     0x008 => "DeviceID",
                0x00C => "VendorID",    0x010 => "DeviceFeatures", 0x034 => "QueueNumMax",
                0x038 => "QueueNum",    0x044 => "QueueReady",  0x060 => "IRQStatus",
                0x070 => "Status",      0x0FC => "ConfigGen",   0x100 => "Capacity_lo",
                0x104 => "Capacity_hi", _ => "?",
            };
            eprintln!("[virtio] load  @{off:#05x} ({name}) = {v:#010x}");
        }
        v
    }

    // Returns true when QueueNotify is written — caller must call process_queue().
    pub fn store(&mut self, off: u64, _width: usize, val: u64) -> bool {
        if self.debug {
            let name = match off {
                0x014 => "DevFeatSel", 0x020 => "DrvFeatures", 0x024 => "DrvFeatSel",
                0x030 => "QueueSel",   0x038 => "QueueNum",     0x044 => "QueueReady",
                0x050 => "QueueNotify",0x064 => "IRQAck",       0x070 => "Status",
                0x080 => "DescLo",     0x084 => "DescHi",
                0x090 => "AvailLo",    0x094 => "AvailHi",
                0x0a0 => "UsedLo",     0x0a4 => "UsedHi",
                _ => "?",
            };
            eprintln!("[virtio] store @{off:#05x} ({name}) = {val:#010x}");
        }
        match off {
            0x014 => self.device_features_sel = val as u32,
            0x020 => {
                let sel = self.driver_features_sel as usize;
                if sel < 2 { self.driver_features[sel] = val as u32; }
            }
            0x024 => self.driver_features_sel = val as u32,
            0x030 => {}  // QueueSel: single queue, ignore
            0x038 => self.queue_num = (val as u32).min(QUEUE_SIZE as u32),
            0x044 => self.queue_ready = val as u32,
            0x050 => return true,  // QueueNotify
            0x064 => self.irq_status &= !(val as u32),  // InterruptACK
            0x070 => {
                self.device_status = val as u32;
                if val == 0 { self.reset(); }
            }
            // QueueDescLow/High — virtio_mmio.h 0x080/0x084
            0x080 => self.desc_addr  = (self.desc_addr  & 0xFFFF_FFFF_0000_0000) | (val & 0xFFFF_FFFF),
            0x084 => self.desc_addr  = (self.desc_addr  & 0x0000_0000_FFFF_FFFF) | (val << 32),
            // QueueAvailLow/High (driver ring) — virtio_mmio.h 0x090/0x094
            0x090 => self.avail_addr = (self.avail_addr & 0xFFFF_FFFF_0000_0000) | (val & 0xFFFF_FFFF),
            0x094 => self.avail_addr = (self.avail_addr & 0x0000_0000_FFFF_FFFF) | (val << 32),
            // QueueUsedLow/High (device ring) — virtio_mmio.h 0x0a0/0x0a4
            0x0a0 => self.used_addr  = (self.used_addr  & 0xFFFF_FFFF_0000_0000) | (val & 0xFFFF_FFFF),
            0x0a4 => self.used_addr  = (self.used_addr  & 0x0000_0000_FFFF_FFFF) | (val << 32),
            _ => {}
        }
        false
    }

    fn reset(&mut self) {
        eprintln!("[virtio] device reset (device_status written 0)");
        self.queue_ready = 0;
        self.desc_addr   = 0;
        self.avail_addr  = 0;
        self.used_addr   = 0;
        self.irq_status  = 0;
        self.last_avail_idx = 0;
    }

    // Called by Bus when QueueNotify is written.  Drains all pending entries in
    // the available ring, performs the I/O synchronously, then writes results
    // into the used ring and sets irq_status so the PLIC can raise an interrupt.
    pub fn process_queue(&mut self, ram: &mut [u8], ram_base: u64) {
        if self.queue_ready == 0 || self.avail_addr == 0 || self.desc_addr == 0 {
            eprintln!("[virtio] WARN: QueueNotify while not ready \
                       (ready={} avail={:#x} desc={:#x} used={:#x})",
                      self.queue_ready, self.avail_addr, self.desc_addr, self.used_addr);
            return;
        }
        if self.debug {
            eprintln!("[virtio] process_queue: desc={:#x} avail={:#x} used={:#x}",
                self.desc_addr, self.avail_addr, self.used_addr);
        }

        let mut processed = 0u32;
        loop {
            // driver ring: u16 flags @ +0, u16 idx @ +2, u16 ring[N] @ +4
            let avail_idx = r16(ram, ram_base, self.avail_addr + 2);
            if self.last_avail_idx == avail_idx { break; }

            let slot = (self.last_avail_idx % QUEUE_SIZE) as u64;
            let head = r16(ram, ram_base, self.avail_addr + 4 + slot * 2) as usize;

            self.do_request(ram, ram_base, head);

            // device ring: u16 flags @ +0, u16 idx @ +2, used_elem[N]{u32 id, u32 len} @ +4
            let used_idx = r16(ram, ram_base, self.used_addr + 2);
            let elem = self.used_addr + 4 + (used_idx % QUEUE_SIZE) as u64 * 8;
            w32(ram, ram_base, elem,     head as u32); // id
            w32(ram, ram_base, elem + 4, 0);           // len (bytes written; 0 is fine for blk)
            w16(ram, ram_base, self.used_addr + 2, used_idx.wrapping_add(1));

            self.last_avail_idx = self.last_avail_idx.wrapping_add(1);
            self.irq_status = 1;
            processed += 1;
        }
        if processed == 0 {
            eprintln!("[virtio] WARN: QueueNotify but avail ring empty \
                       (avail_idx={} last={})",
                      r16(ram, ram_base, self.avail_addr + 2),
                      self.last_avail_idx);
        }
    }

    fn do_request(&mut self, ram: &mut [u8], ram_base: u64, head: usize) {
        // Collect the descriptor chain (cap at 16 to guard against cycles).
        let mut descs: Vec<(u64, u32, u16)> = Vec::with_capacity(4); // (addr, len, flags)
        let mut idx = head;
        for _ in 0..16 {
            let base = self.desc_addr + idx as u64 * 16;
            let addr  = r64(ram, ram_base, base);
            let len   = r32(ram, ram_base, base + 8);
            let flags = r16(ram, ram_base, base + 12);
            let next  = r16(ram, ram_base, base + 14);
            descs.push((addr, len, flags));
            if flags & DESC_F_NEXT == 0 { break; }
            idx = next as usize;
        }

        if descs.len() < 2 { return; }

        // Header descriptor: { u32 type, u32 reserved, u64 sector }
        let hdr     = descs[0].0;
        let req_type = r32(ram, ram_base, hdr);
        let sector   = r64(ram, ram_base, hdr + 8);

        let status_addr = descs[descs.len() - 1].0;
        let data_descs  = &descs[1..descs.len() - 1];

        if self.disk.is_none() {
            w8(ram, ram_base, status_addr, S_IOERR);
            return;
        }

        let byte_off = sector * SECTOR_SIZE;
        if self.debug {
            let type_str = match req_type { T_IN => "READ", T_OUT => "WRITE", T_FLUSH => "FLUSH", _ => "?" };
            eprintln!("[virtio] req type={type_str} sector={sector} byte_off={byte_off:#x} descs={}", descs.len());
            for (i, &(addr, len, flags)) in descs.iter().enumerate() {
                eprintln!("[virtio]   desc[{i}] addr={addr:#x} len={len} flags={flags:#x}");
            }
        }
        let ok = match req_type {
            T_IN => {
                // Read from disk into device-writable data descriptors.
                let disk = self.disk.as_mut().unwrap();
                if disk.seek(SeekFrom::Start(byte_off)).is_err() {
                    if self.debug { eprintln!("[virtio] seek FAIL sector={sector}"); }
                    w8(ram, ram_base, status_addr, S_IOERR);
                    return;
                }
                let mut success = true;
                for &(addr, len, flags) in data_descs {
                    if flags & DESC_F_WRITE == 0 { continue; }
                    if addr < ram_base || addr - ram_base > ram.len() as u64 - len as u64 {
                        if self.debug { eprintln!("[virtio] T_IN addr {addr:#x} out of RAM"); }
                        success = false;
                        break;
                    }
                    let off = (addr - ram_base) as usize;
                    match disk.read_exact(&mut ram[off..off + len as usize]) {
                        Ok(()) => {
                            if self.debug {
                                let preview = &ram[off..off + len.min(16) as usize];
                                eprintln!("[virtio] T_IN ok sector={sector} len={len} preview={preview:02x?}");
                            }
                        }
                        Err(e) => {
                            if self.debug { eprintln!("[virtio] T_IN read_exact err sector={sector}: {e}"); }
                            success = false;
                            break;
                        }
                    }
                }
                success
            }
            T_OUT => {
                // Write host-readable data descriptors to disk.
                let disk = self.disk.as_mut().unwrap();
                if disk.seek(SeekFrom::Start(byte_off)).is_err() {
                    if self.debug { eprintln!("[virtio] seek FAIL sector={sector}"); }
                    w8(ram, ram_base, status_addr, S_IOERR);
                    return;
                }
                let mut success = true;
                for &(addr, len, flags) in data_descs {
                    if flags & DESC_F_WRITE != 0 { continue; }
                    if addr < ram_base || addr - ram_base > ram.len() as u64 - len as u64 {
                        if self.debug { eprintln!("[virtio] T_OUT addr {addr:#x} out of RAM"); }
                        success = false;
                        break;
                    }
                    let off = (addr - ram_base) as usize;
                    if self.debug {
                        eprintln!("[virtio] T_OUT sector={sector} len={len} addr={addr:#x}");
                    }
                    if disk.write_all(&ram[off..off + len as usize]).is_err() {
                        success = false;
                        break;
                    }
                }
                success
            }
            T_FLUSH => true, // synchronous I/O — already durable
            _ => false,
        };

        w8(ram, ram_base, status_addr, if ok { S_OK } else { S_IOERR });
    }
}

// ── Guest RAM helpers ──────────────────────────────────────────────────────

fn r16(ram: &[u8], base: u64, addr: u64) -> u16 {
    let o = (addr - base) as usize;
    u16::from_le_bytes(ram[o..o + 2].try_into().unwrap())
}
fn r32(ram: &[u8], base: u64, addr: u64) -> u32 {
    let o = (addr - base) as usize;
    u32::from_le_bytes(ram[o..o + 4].try_into().unwrap())
}
fn r64(ram: &[u8], base: u64, addr: u64) -> u64 {
    let o = (addr - base) as usize;
    u64::from_le_bytes(ram[o..o + 8].try_into().unwrap())
}
fn w8(ram: &mut [u8], base: u64, addr: u64, val: u8) {
    ram[(addr - base) as usize] = val;
}
fn w16(ram: &mut [u8], base: u64, addr: u64, val: u16) {
    let o = (addr - base) as usize;
    ram[o..o + 2].copy_from_slice(&val.to_le_bytes());
}
fn w32(ram: &mut [u8], base: u64, addr: u64, val: u32) {
    let o = (addr - base) as usize;
    ram[o..o + 4].copy_from_slice(&val.to_le_bytes());
}
