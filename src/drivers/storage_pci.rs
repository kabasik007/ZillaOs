//! Passive PCI mass-storage controller inventory, **NOT** a disk driver.
//!
//! Observes only vendor/device/class/multifunction headers via PCI config.
//! No BAR probing, DMA, interrupts, controller reset, ATA/NVMe commands,
//! logical disk access, partition scanning, formatting or sector writes.
//! Only the early single-CPU QEMU/VMware research ISO calls this module.

pub const MAX_STORAGE_CONTROLLERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciLocation {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciLocation {
    /// PCI configuration mechanism #1 DWORD address for segment group 0.
    /// The caller must perform 32-bit I/O: OUT 0xCF8 then IN 0xCFC.
    pub const fn address(self, offset: u8) -> u32 {
        0x8000_0000
            | ((self.bus as u32) << 16)
            | ((self.device as u32) << 11)
            | ((self.function as u32) << 8)
            | ((offset as u32) & 0xfc)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageKind {
    Scsi,
    Ide,
    SataAhci,
    Nvme,
    Raid,
    Other,
}

impl StorageKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Scsi => "SCSI",
            Self::Ide => "IDE",
            Self::SataAhci => "SATA / AHCI",
            Self::Nvme => "NVME",
            Self::Raid => "RAID",
            Self::Other => "MASS STORAGE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageController {
    pub location: PciLocation,
    pub vendor: u16,
    pub device_id: u16,
    pub subclass: u8,
    pub programming_interface: u8,
    pub kind: StorageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageInventory {
    pub controllers: [Option<StorageController>; MAX_STORAGE_CONTROLLERS],
    pub count: usize,
    /// True means additional controllers were found but not stored.
    pub truncated: bool,
}

impl Default for StorageInventory {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageInventory {
    pub const fn new() -> Self {
        Self {
            controllers: [None; MAX_STORAGE_CONTROLLERS],
            count: 0,
            truncated: false,
        }
    }

    pub fn push(&mut self, controller: StorageController) {
        if self.count < self.controllers.len() {
            self.controllers[self.count] = Some(controller);
            self.count += 1;
        } else {
            self.truncated = true;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = StorageController> + '_ {
        self.controllers[..self.count].iter().flatten().copied()
    }

    /// Deliberately NOT a disk count. PCI functions may host multiple drives,
    /// zero drives, virtual CD-ROMs or a mix. Without a block driver, the
    /// number of selectable disks is always zero.
    pub const fn selectable_disks(&self) -> usize {
        0
    }
}

pub fn decode_kind(class: u8, subclass: u8, prog_if: u8) -> Option<StorageKind> {
    if class != 0x01 {
        return None;
    }
    Some(match subclass {
        0x00 => StorageKind::Scsi,
        0x01 => StorageKind::Ide,
        0x04 => StorageKind::Raid,
        0x06 if prog_if == 0x01 => StorageKind::SataAhci,
        0x08 if prog_if == 0x02 => StorageKind::Nvme,
        _ => StorageKind::Other,
    })
}

/// Enumerate segment 0, buses 0..=255, slots 0..31 and only valid functions.
/// `read` must be a **read-only config-space adapter**; it receives a
/// location and a DWORD register offset (0x00, 0x08 or 0x0c).
/// Never assume that a controller proves a disk is attached.
pub fn enumerate_storage(mut read: impl FnMut(PciLocation, u8) -> u32) -> StorageInventory {
    let mut found = StorageInventory::new();
    for bus in 0..=u8::MAX {
        for device in 0..32u8 {
            let zero = PciLocation { bus, device, function: 0 };
            let identity = read(zero, 0x00);
            if identity & 0xffff == 0xffff || identity & 0xffff == 0 {
                continue;
            }
            let header = ((read(zero, 0x0c) >> 16) & 0xff) as u8;
            let max_functions = if header & 0x80 != 0 { 8 } else { 1 };
            for function in 0..max_functions {
                let loc = PciLocation { bus, device, function };
                let id = if function == 0 { identity } else { read(loc, 0x00) };
                let vendor = (id & 0xffff) as u16;
                if vendor == 0xffff || vendor == 0 {
                    continue;
                }
                let class_register = read(loc, 0x08);
                let class = (class_register >> 24) as u8;
                let subclass = (class_register >> 16) as u8;
                let prog_if = (class_register >> 8) as u8;
                if let Some(kind) = decode_kind(class, subclass, prog_if) {
                    found.push(StorageController {
                        location: loc,
                        vendor,
                        device_id: (id >> 16) as u16,
                        subclass,
                        programming_interface: prog_if,
                        kind,
                    });
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: u16, kind: StorageKind) -> StorageController {
        StorageController {
            location: PciLocation { bus: 0, device: 1, function: 0 },
            vendor: 0x15ad,
            device_id: id,
            subclass: 6,
            programming_interface: 1,
            kind,
        }
    }

    #[test]
    fn pci_config_address_uses_correct_bus_device_function_bits() {
        let slot = PciLocation { bus: 0x13, device: 0x1c, function: 5 };
        assert_eq!(slot.address(0x08), 0x8013_e508);
        assert_eq!(slot.address(0x0b), slot.address(0x08));
    }

    #[test]
    fn correctly_classifies_ahci_nvme_scsi_and_nonstorage() {
        assert_eq!(decode_kind(1, 6, 1), Some(StorageKind::SataAhci));
        assert_eq!(decode_kind(1, 8, 2), Some(StorageKind::Nvme));
        assert_eq!(decode_kind(1, 0, 0), Some(StorageKind::Scsi));
        assert_eq!(decode_kind(1, 4, 0), Some(StorageKind::Raid));
        assert_eq!(decode_kind(1, 6, 0), Some(StorageKind::Other));
        assert_eq!(decode_kind(3, 0, 0), None);
    }

    #[test]
    fn enumerates_only_storage_functions_without_reading_bars() {
        let mut offsets = [0usize; 16];
        let seen = enumerate_storage(|loc, offset| {
            assert!(matches!(offset, 0x00 | 0x08 | 0x0c));
            offsets[loc.function as usize] += 1;
            if loc.bus == 0 && loc.device == 3 && loc.function == 0 {
                match offset {
                    0x00 => 0x07e0_15ad,
                    0x08 => 0x0106_0100,
                    0x0c => 0,
                    _ => unreachable!(),
                }
            } else {
                u32::MAX
            }
        });
        assert_eq!(seen.count, 1);
        assert_eq!(seen.iter().next().unwrap().kind, StorageKind::SataAhci);
        assert_eq!(seen.selectable_disks(), 0);
        assert_eq!(offsets[1], 0); // not a multifunction controller
    }

    #[test]
    fn multifunction_discovery_can_reach_nonzero_function() {
        let inv = enumerate_storage(|loc, offset| {
            if loc.bus != 0 || loc.device != 2 { return u32::MAX; }
            match (loc.function, offset) {
                (0, 0x00) => 0x0711_15ad,
                (0, 0x0c) => 0x0080_0000,
                (0, 0x08) => 0x0300_0000,
                (1, 0x00) => 0x07f0_15ad,
                (1, 0x08) => 0x0108_0200,
                _ => u32::MAX,
            }
        });
        assert_eq!(inv.count, 1);
        let controller = inv.iter().next().unwrap();
        assert_eq!(controller.location.function, 1);
        assert_eq!(controller.kind, StorageKind::Nvme);
    }

    #[test]
    fn overflow_keeps_fixed_memory_capacity_without_fabricating_disks() {
        let mut inv = StorageInventory::new();
        for id in 0..13 {
            inv.push(sample(id, StorageKind::Scsi));
        }
        assert_eq!(inv.count, MAX_STORAGE_CONTROLLERS);
        assert!(inv.truncated);
        assert_eq!(inv.selectable_disks(), 0);
    }
}
