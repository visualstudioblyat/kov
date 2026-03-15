use std::collections::HashMap;

pub const RAM_BASE: u32 = 0x2000_0000;
pub const RAM_SIZE: u32 = 32 * 1024;
pub const FLASH_BASE: u32 = 0x0800_0000;
pub const FLASH_SIZE: u32 = 128 * 1024;

// MMIO region — reads/writes here are logged for verification
pub const MMIO_BASE: u32 = 0x4000_0000;
pub const MMIO_SIZE: u32 = 0x4000_0000;

pub struct Memory {
    ram: Vec<u8>,
    flash: Vec<u8>,
    pub flash_base: u32,
    pub ram_base: u32,
    pub mmio_log: Vec<MmioAccess>,
    mmio_shadow: HashMap<u32, u32>,
}

#[derive(Debug, Clone)]
pub struct MmioAccess {
    pub address: u32,
    pub value: u32,
    pub width: u32,
    pub is_write: bool,
}

impl Memory {
    pub fn new() -> Self {
        Self::with_bases(FLASH_BASE, RAM_BASE)
    }

    pub fn with_bases(flash_base: u32, ram_base: u32) -> Self {
        Self {
            ram: vec![0u8; RAM_SIZE as usize],
            flash: vec![0u8; FLASH_SIZE as usize],
            flash_base,
            ram_base,
            mmio_log: Vec::new(),
            mmio_shadow: HashMap::new(),
        }
    }

    pub fn load_flash(&mut self, data: &[u8]) {
        let len = data.len().min(FLASH_SIZE as usize);
        self.flash[..len].copy_from_slice(&data[..len]);
    }

    pub fn read8(&mut self, addr: u32) -> u8 {
        if let Some(off) = self.flash_offset(addr) {
            self.flash[off]
        } else if let Some(off) = self.ram_offset(addr) {
            self.ram[off]
        } else if self.is_mmio(addr) {
            let val = self.mmio_shadow.get(&(addr & !3)).copied().unwrap_or(0);
            let shift = (addr & 3) * 8;
            ((val >> shift) & 0xFF) as u8
        } else {
            0
        }
    }

    pub fn read16(&mut self, addr: u32) -> u16 {
        self.read8(addr) as u16 | ((self.read8(addr + 1) as u16) << 8)
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        if let Some(off) = self.flash_offset(addr) {
            u32::from_le_bytes([
                self.flash[off], self.flash[off + 1],
                self.flash[off + 2], self.flash[off + 3],
            ])
        } else if let Some(off) = self.ram_offset(addr) {
            u32::from_le_bytes([
                self.ram[off], self.ram[off + 1],
                self.ram[off + 2], self.ram[off + 3],
            ])
        } else if self.is_mmio(addr) {
            self.mmio_log.push(MmioAccess {
                address: addr, value: 0, width: 4, is_write: false,
            });
            self.mmio_shadow.get(&addr).copied().unwrap_or(0)
        } else {
            0
        }
    }

    pub fn write8(&mut self, addr: u32, val: u8) {
        if let Some(off) = self.ram_offset(addr) {
            self.ram[off] = val;
        } else if self.is_mmio(addr) {
            self.mmio_log.push(MmioAccess {
                address: addr, value: val as u32, width: 1, is_write: true,
            });
            self.mmio_shadow.insert(addr, val as u32);
        }
    }

    pub fn write16(&mut self, addr: u32, val: u16) {
        self.write8(addr, val as u8);
        self.write8(addr + 1, (val >> 8) as u8);
    }

    pub fn write32(&mut self, addr: u32, val: u32) {
        if let Some(off) = self.ram_offset(addr) {
            self.ram[off..off + 4].copy_from_slice(&val.to_le_bytes());
        } else if self.is_mmio(addr) {
            self.mmio_log.push(MmioAccess {
                address: addr, value: val, width: 4, is_write: true,
            });
            self.mmio_shadow.insert(addr, val);
        }
    }

    fn flash_offset(&self, addr: u32) -> Option<usize> {
        if addr >= self.flash_base && addr < self.flash_base + FLASH_SIZE {
            Some((addr - self.flash_base) as usize)
        } else {
            None
        }
    }

    fn ram_offset(&self, addr: u32) -> Option<usize> {
        if addr >= self.ram_base && addr < self.ram_base + RAM_SIZE {
            Some((addr - self.ram_base) as usize)
        } else {
            None
        }
    }

    fn is_mmio(&self, addr: u32) -> bool {
        addr >= MMIO_BASE || addr >= 0x6000_0000
    }
}
