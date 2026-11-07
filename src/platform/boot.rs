//! Single early-boot authority for PCI identities and generation-safe handles.
//! One bounded read-only segment-0 scan. Drivers consume cached, immutable data.
//! No global mutable singleton, second scanner, config writes, MMIO or DMA.
#![forbid(unsafe_code)]

use super::device::{DeviceRegistry, DeviceSnapshot};
use super::pci::{PciAddress, PciConfigRead, PciIdentity, ScanReport, scan_legacy_segment0};
use crate::storage_pci::{PciLocation, StorageController, StorageInventory, decode_kind};
use crate::video::VideoAdapter;

pub const MAX_BOOT_PCI_DEVICES: usize = 64;
pub const MAX_VMWARE_PCI_HEADERS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootScanStatus {
    Complete,
    InventoryOverflow,
    MalformedTopology,
    ConfigUnavailable,
    RegistryRejected,
    VmwareHeaderOverflow,
}

impl BootScanStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Complete => "COMPLETE",
            Self::InventoryOverflow => "INVENTORY_OVERFLOW",
            Self::MalformedTopology => "MALFORMED_TOPOLOGY",
            Self::ConfigUnavailable => "CONFIG_UNAVAILABLE",
            Self::RegistryRejected => "REGISTRY_REJECTED",
            Self::VmwareHeaderOverflow => "VMWARE_HEADER_OVERFLOW",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct VmwareConfig {
    identity: PciIdentity,
    command_status: u32,
    bar0: u32,
    bar1: u32,
    bar2: u32,
}

/// Authoritative read-only device registry for this boot. The PCI inventory
/// is temporary during discovery and discarded after transactional import.
/// A future SMP kernel must put the owning gateway behind appropriate locks.
pub struct BootPci<const DEVICES: usize> {
    registry: DeviceRegistry<DEVICES, 0>,
    vmware: [Option<VmwareConfig>; MAX_VMWARE_PCI_HEADERS],
    pub report: ScanReport,
    pub status: BootScanStatus,
}

impl<const DEVICES: usize> BootPci<DEVICES> {
    fn empty() -> Self {
        Self {
            registry: DeviceRegistry::new(),
            vmware: [None; MAX_VMWARE_PCI_HEADERS],
            report: ScanReport::default(),
            status: BootScanStatus::RegistryRejected,
        }
    }

    pub const fn ready(&self) -> bool {
        matches!(self.status, BootScanStatus::Complete)
    }

    pub fn registry(&self) -> Option<&DeviceRegistry<DEVICES, 0>> {
        if self.ready() {
            Some(&self.registry)
        } else {
            None
        }
    }

    pub fn snapshot(&self) -> Option<DeviceSnapshot<DEVICES>> {
        self.registry().map(DeviceRegistry::snapshot)
    }

    /// A single segment-0 CF8/CFC read-only pass over each legacy bus 0..=last_bus.
    /// Full range by default: old storage/video scanners inspected all buses
    /// even where ACPI host bridge discovery was not available yet.
    pub fn discover(config: &mut impl PciConfigRead, last_bus: u8) -> Self {
        let mut boot = Self::empty();
        let scan = scan_legacy_segment0::<DEVICES>(config, last_bus);
        boot.report = scan.report;
        if scan.truncated {
            boot.status = BootScanStatus::InventoryOverflow;
            return boot;
        }
        if scan.report.config_errors > 0 {
            boot.status = BootScanStatus::ConfigUnavailable;
            return boot;
        }
        if scan.report.malformed_headers > 0 || scan.report.malformed_bridges > 0 {
            boot.status = BootScanStatus::MalformedTopology;
            return boot;
        }
        if boot.registry.import_inventory(&scan).is_err() {
            boot.status = BootScanStatus::RegistryRejected;
            return boot;
        }

        // Cache VMware's PCI *read-only* decoder/BAR registers once. These
        // values are not verified BAR extents and authorize no mapping.
        // The unchanged legacy VMware probe later reads this cache, NOT CF8.
        let mut count = 0usize;
        for info in scan
            .iter()
            .filter(|entry| entry.identity.is_vmware_svga_ii())
        {
            if info.header_type != 0 || count == MAX_VMWARE_PCI_HEADERS {
                boot.status = BootScanStatus::VmwareHeaderOverflow;
                break;
            }
            let address = info.identity.address;
            let fields = [
                config.read32(address, 0x04),
                config.read32(address, 0x10),
                config.read32(address, 0x14),
                config.read32(address, 0x18),
            ];
            let [Ok(command_status), Ok(bar0), Ok(bar1), Ok(bar2)] = fields else {
                boot.status = BootScanStatus::ConfigUnavailable;
                break;
            };
            boot.vmware[count] = Some(VmwareConfig {
                identity: info.identity,
                command_status,
                bar0,
                bar1,
                bar2,
            });
            count += 1;
        }
        if !matches!(boot.status, BootScanStatus::RegistryRejected) {
            // Set only after the entire scan and VMware cache validated.
            // Otherwise all client views remain fail-closed.
            return boot;
        }
        boot.status = BootScanStatus::Complete;
        boot
    }

    /// Storage controller information derived from the same DeviceRegistry.
    /// No PCI read, disk command, BAR sizing or DMA can occur.
    pub fn storage_inventory(&self) -> StorageInventory {
        let mut result = StorageInventory::new();
        if let Some(snapshot) = self.snapshot() {
            for view in snapshot.iter() {
                let identity = view.info.identity;
                if let Some(kind) = decode_kind(
                    identity.class.base,
                    identity.class.subclass,
                    identity.class.programming_interface,
                ) {
                    if let Some(location) = identity.address.legacy_location() {
                        result.push(StorageController {
                            location,
                            vendor: identity.vendor_id,
                            device_id: identity.device_id,
                            subclass: identity.class.subclass,
                            programming_interface: identity.class.programming_interface,
                            kind,
                        });
                    }
                }
            }
        }
        result
    }

    /// The desktop displays only the identity supported by its existing UI.
    /// Non-VMware display controllers remain informational / no acceleration.
    pub fn video_adapter(&self) -> VideoAdapter {
        let mut generic = false;
        if let Some(snapshot) = self.snapshot() {
            for view in snapshot.iter() {
                let id = view.info.identity;
                if id.class.is_display() {
                    if id.is_vmware_svga_ii() && view.info.header_type == 0 {
                        return VideoAdapter::VmwareSvgaII;
                    }
                    generic = true;
                }
            }
        }
        if generic {
            VideoAdapter::GenericPciDisplay
        } else {
            VideoAdapter::FirmwareOnly
        }
    }

    /// Legacy VMware adapter. Emulates only dwords consumed by the existing
    /// read-only SVGA discovery and maps everything else to absent/unsupported.
    /// This does not call a hardware config reader or enumerate the bus.
    pub fn legacy_read32(&self, location: PciLocation, offset: u8) -> u32 {
        if !self.ready() || offset & 3 != 0 || offset > 0xfc {
            return u32::MAX;
        }
        let address = PciAddress::from(location);
        let Some(view) = self
            .registry
            .find(address)
            .and_then(|id| self.registry.get(id).ok())
        else {
            return u32::MAX;
        };
        let pci = view.info;
        let id = pci.identity;
        let cached = self
            .vmware
            .iter()
            .flatten()
            .find(|candidate| candidate.identity.address == address);
        match offset {
            0 => ((id.device_id as u32) << 16) | id.vendor_id as u32,
            4 => cached.map_or(u32::MAX, |c| c.command_status),
            8 => {
                ((id.class.base as u32) << 24)
                    | ((id.class.subclass as u32) << 16)
                    | ((id.class.programming_interface as u32) << 8)
                    | id.revision as u32
            }
            12 => ((pci.header_type | if pci.multifunction { 0x80 } else { 0 }) as u32) << 16,
            16 => cached.map_or(u32::MAX, |c| c.bar0),
            20 => cached.map_or(u32::MAX, |c| c.bar1),
            24 => cached.map_or(u32::MAX, |c| c.bar2),
            44 => match (id.subsystem_vendor_id, id.subsystem_device_id) {
                (Some(v), Some(d)) => ((d as u32) << 16) | v as u32,
                _ => u32::MAX,
            },
            _ => u32::MAX,
        }
    }

    pub fn verified_resource_claims_available(&self) -> bool {
        // No firmware resource authority, BAR sizing, IRQ or DMA here.
        false
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::super::pci::{ConfigError, MockConfig};
    use super::*;
    use crate::video::gpu::vmware_svga::pci::locate;
    use core::cell::Cell;

    fn fixture(addr: PciAddress, off: u16) -> Result<u32, ConfigError> {
        if addr.segment != 0 {
            return Err(ConfigError::UnsupportedSegment);
        }
        match (addr.bus, addr.device, addr.function, off) {
            (0, 2, 0, 0) => Ok(0x2922_8086),
            (0, 2, 0, 0x08) => Ok(0x0106_0101), // AHCI
            (0, 2, 0, 0x0c) => Ok(0),
            (0, 15, 0, 0) => Ok(0x0405_15ad),
            (0, 15, 0, 0x08) => Ok(0x0300_0001),
            (0, 15, 0, 0x0c) => Ok(0),
            (0, 15, 0, 0x04) => Ok(3),
            (0, 15, 0, 0x10) => Ok(0x1071),
            (0, 15, 0, 0x14) => Ok(0xe000_0008),
            (0, 15, 0, 0x18) => Ok(0xe800_0000),
            (0, 16, 0, 0) => Ok(0x73bf_1002),
            (0, 16, 0, 0x08) => Ok(0x0300_00a1),
            (0, 16, 0, 0x0c) => Ok(0),
            (0, 16, 0, 0x2c) => Ok(0xe438_1da2),
            (_, _, _, 0x2c) => Ok(u32::MAX),
            _ => Ok(u32::MAX),
        }
    }

    #[test]
    fn single_hardware_scan_feeds_storage_vmware_and_amd_identity() {
        let reads = Cell::new(0);
        let mut backend = MockConfig::new(|addr, off| {
            reads.set(reads.get() + 1);
            fixture(addr, off)
        });
        let boot = BootPci::<8>::discover(&mut backend, 0);
        assert_eq!(boot.status, BootScanStatus::Complete);
        assert_eq!(boot.report.visited_buses, 1);
        let after_scan = reads.get();
        let storage = boot.storage_inventory();
        assert_eq!(storage.count, 1);
        assert_eq!(
            storage.iter().next().unwrap().kind,
            crate::storage_pci::StorageKind::SataAhci
        );
        assert_eq!(boot.video_adapter(), VideoAdapter::VmwareSvgaII);
        let info = locate(|loc, off| boot.legacy_read32(loc, off))
            .unwrap()
            .unwrap();
        assert_eq!(info.io_base, 0x1070);
        let reg = boot.registry().unwrap();
        let amd = reg.find(PciAddress::new(0, 0, 16, 0).unwrap()).unwrap();
        assert!(
            reg.get(amd)
                .unwrap()
                .info
                .identity
                .is_amd_display_candidate()
        );
        assert_eq!(reads.get(), after_scan); // cached clients do not touch hardware
        assert!(!boot.verified_resource_claims_available());
    }

    #[test]
    fn corrupt_header_and_scan_overflow_deny_all_clients() {
        let mut corrupt = MockConfig::new(|addr: PciAddress, off: u16| {
            if addr.bus == 0 && addr.device == 3 {
                Ok(match off {
                    0 => 0x1234_8086,
                    0x0c => 0x007f_0000,
                    _ => 0,
                })
            } else {
                Ok(u32::MAX)
            }
        });
        let denied = BootPci::<8>::discover(&mut corrupt, 0);
        assert_eq!(denied.status, BootScanStatus::MalformedTopology);
        assert_eq!(denied.storage_inventory().count, 0);
        assert_eq!(
            denied.legacy_read32(
                PciLocation {
                    bus: 0,
                    device: 3,
                    function: 0
                },
                0
            ),
            u32::MAX
        );

        let mut dense = MockConfig::new(|addr: PciAddress, off: u16| {
            if addr.bus == 0 && addr.device < 5 && addr.function == 0 {
                Ok(match off {
                    0 => 0x1234_8086,
                    0x0c => 0,
                    0x08 => 0x0106_0100,
                    _ => u32::MAX,
                })
            } else {
                Ok(u32::MAX)
            }
        });
        let overflow = BootPci::<2>::discover(&mut dense, 0);
        assert_eq!(overflow.status, BootScanStatus::InventoryOverflow);
        assert!(overflow.registry().is_none());
        assert_eq!(overflow.storage_inventory().count, 0);
    }

    #[test]
    fn invalid_bridge_range_denies_all_device_views() {
        let mut backend = MockConfig::new(|addr: PciAddress, off: u16| {
            if addr.bus == 0 && addr.device == 1 {
                Ok(match off {
                    0 => 0x1234_8086,
                    0x0c => 0x0001_0000,
                    0x08 => 0x0604_0000,
                    0x18 => 0x0001_0001, // primary 1, secondary 0: malformed
                    _ => u32::MAX,
                })
            } else {
                fixture(addr, off)
            }
        });
        let boot = BootPci::<8>::discover(&mut backend, 0);
        assert_eq!(boot.status, BootScanStatus::MalformedTopology);
        assert!(boot.registry().is_none());
        assert_eq!(boot.storage_inventory().count, 0);
        assert_eq!(boot.video_adapter(), VideoAdapter::FirmwareOnly);
    }

    #[test]
    fn faulted_config_fails_closed_without_reporting_partial_devices() {
        let mut backend = MockConfig::new(|addr: PciAddress, off: u16| {
            if addr.bus == 0 && addr.device == 2 && off == 0x0c {
                Err(ConfigError::Unavailable)
            } else {
                fixture(addr, off)
            }
        });
        let denied = BootPci::<8>::discover(&mut backend, 0);
        assert_eq!(denied.status, BootScanStatus::ConfigUnavailable);
        assert_eq!(denied.video_adapter(), VideoAdapter::FirmwareOnly);
        assert!(denied.registry().is_none());
    }
}
