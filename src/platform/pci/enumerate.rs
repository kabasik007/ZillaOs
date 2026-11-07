//! Bounded bridge-aware PCI discovery. No allocation and no config writes.
use super::config::PciConfigRead;
use super::identity::{PciAddress, PciBridge, PciClass, PciDeviceInfo, PciIdentity};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub visited_buses: usize,
    pub malformed_headers: usize,
    pub malformed_bridges: usize,
    pub config_errors: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    InvalidBusRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciInventory<const CAP: usize> {
    devices: [Option<PciDeviceInfo>; CAP],
    pub count: usize,
    pub truncated: bool,
    pub report: ScanReport,
}

impl<const CAP: usize> PciInventory<CAP> {
    pub const fn new() -> Self {
        Self {
            devices: [None; CAP],
            count: 0,
            truncated: false,
            report: ScanReport {
                visited_buses: 0,
                malformed_headers: 0,
                malformed_bridges: 0,
                config_errors: 0,
            },
        }
    }

    fn push(&mut self, info: PciDeviceInfo) {
        if self.count == CAP {
            self.truncated = true;
        } else {
            self.devices[self.count] = Some(info);
            self.count += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = PciDeviceInfo> + '_ {
        self.devices[..self.count].iter().flatten().copied()
    }
}

impl<const CAP: usize> Default for PciInventory<CAP> {
    fn default() -> Self { Self::new() }
}

fn read(
    config: &mut impl PciConfigRead,
    address: PciAddress,
    offset: u16,
    report: &mut ScanReport,
) -> Option<u32> {
    match config.read32(address, offset) {
        Ok(value) => Some(value),
        Err(_) => {
            report.config_errors += 1;
            None
        }
    }
}

fn is_present(id: u32) -> bool {
    let vendor = id as u16;
    vendor != 0 && vendor != 0xffff
}

/// Follow bridge secondary buses from one known root in one PCI segment.
/// Only the root and reachable buses are probed; use firmware host bridge
/// topology to select additional roots. `last_bus` is a hard scan ceiling.
///
/// A malformed bridge is reported and NOT traversed. Every bus is visited
/// at most once (bounded 256-element queue and bitmap), including cycles.
pub fn scan_topology<const CAP: usize>(
    config: &mut impl PciConfigRead,
    segment: u16,
    root_bus: u8,
    last_bus: u8,
) -> Result<PciInventory<CAP>, ScanError> {
    if root_bus > last_bus {
        return Err(ScanError::InvalidBusRange);
    }
    let mut inventory = PciInventory::<CAP>::new();
    let mut visited = [false; 256];
    let mut queue = [0u8; 256];
    let mut front = 0usize;
    let mut back = 1usize;
    queue[0] = root_bus;
    visited[root_bus as usize] = true;

    while front < back {
        let bus = queue[front];
        front += 1;
        inventory.report.visited_buses += 1;
        for device in 0..32u8 {
            let address0 = PciAddress { segment, bus, device, function: 0 };
            let Some(id0) = read(config, address0, 0, &mut inventory.report) else { continue; };
            if !is_present(id0) { continue; }

            let Some(header0) = read(config, address0, 0x0c, &mut inventory.report) else { continue; };
            let function_count = if header0 & 0x0080_0000 != 0 { 8 } else { 1 };
            for function in 0..function_count {
                let address = PciAddress { segment, bus, device, function };
                let id = if function == 0 {
                    id0
                } else {
                    let Some(value) = read(config, address, 0, &mut inventory.report) else { continue; };
                    value
                };
                if !is_present(id) { continue; }
                let header = if function == 0 {
                    header0
                } else {
                    let Some(value) = read(config, address, 0x0c, &mut inventory.report) else { continue; };
                    value
                };
                let header_type = ((header >> 16) as u8) & 0x7f;
                if header_type > 2 {
                    inventory.report.malformed_headers += 1;
                    continue;
                }
                let Some(class_register) = read(config, address, 0x08, &mut inventory.report) else { continue; };
                let class = PciClass {
                    base: (class_register >> 24) as u8,
                    subclass: (class_register >> 16) as u8,
                    programming_interface: (class_register >> 8) as u8,
                };
                let mut subsystem_vendor_id = None;
                let mut subsystem_device_id = None;
                if header_type == 0 {
                    if let Some(sub) = read(config, address, 0x2c, &mut inventory.report) {
                        let vendor = sub as u16;
                        let subdevice = (sub >> 16) as u16;
                        if vendor != 0 && vendor != 0xffff {
                            subsystem_vendor_id = Some(vendor);
                            subsystem_device_id = Some(subdevice);
                        }
                    }
                }

                let mut bridge = None;
                if class.is_pci_bridge() {
                    if header_type != 1 {
                        inventory.report.malformed_bridges += 1;
                    } else if let Some(bus_numbers) = read(config, address, 0x18, &mut inventory.report) {
                        let primary = bus_numbers as u8;
                        let secondary = (bus_numbers >> 8) as u8;
                        let subordinate = (bus_numbers >> 16) as u8;
                        if primary != bus || secondary <= primary || secondary > subordinate
                            || subordinate > last_bus || secondary < root_bus
                        {
                            inventory.report.malformed_bridges += 1;
                        } else {
                            bridge = Some(PciBridge {
                                primary_bus: primary,
                                secondary_bus: secondary,
                                subordinate_bus: subordinate,
                            });
                            if visited[secondary as usize] {
                                // Duplicate upstream paths are not claimed as new buses.
                                inventory.report.malformed_bridges += 1;
                            } else {
                                visited[secondary as usize] = true;
                                queue[back] = secondary;
                                back += 1; // <=256 distinct u8 buses, cannot overflow queue
                            }
                        }
                    }
                }
                inventory.push(PciDeviceInfo {
                    identity: PciIdentity {
                        address,
                        vendor_id: id as u16,
                        device_id: (id >> 16) as u16,
                        revision: class_register as u8,
                        class,
                        subsystem_vendor_id,
                        subsystem_device_id,
                    },
                    header_type,
                    multifunction: ((header >> 23) & 1) != 0,
                    bridge,
                });
            }
        }
    }
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::pci::config::{ConfigError, MockConfig};

    fn fixture(address: PciAddress, offset: u16) -> Result<u32, ConfigError> {
        if address.segment != 0 { return Err(ConfigError::UnsupportedSegment); }
        match (address.bus, address.device, address.function, offset) {
            (0, 1, 0, 0) => Ok(0x1234_15ad),
            (0, 1, 0, 0x0c) => Ok(0x0080_0000), // multi
            (0, 1, 0, 0x08) => Ok(0x0106_0101),
            (0, 1, 1, 0) => Ok(0x0405_15ad),
            (0, 1, 1, 0x0c) => Ok(0),
            (0, 1, 1, 0x08) => Ok(0x0300_0002),
            (0, 2, 0, 0) => Ok(0xabcd_8086),
            (0, 2, 0, 0x0c) => Ok(0x0001_0000), // header type 1
            (0, 2, 0, 0x08) => Ok(0x0604_0001),
            (0, 2, 0, 0x18) => Ok(0x0002_0200), // primary 0, secondary 2, subordinate 2
            (2, 0, 0, 0) => Ok(0x73bf_1002),
            (2, 0, 0, 0x0c) => Ok(0),
            (2, 0, 0, 0x08) => Ok(0x0300_0001),
            (_, _, _, 0x2c) => Ok(u32::MAX),
            _ => Ok(u32::MAX),
        }
    }

    #[test]
    fn follows_bridge_and_multifunction_with_stable_order() {
        let mut mock = MockConfig::new(fixture);
        let inventory = scan_topology::<8>(&mut mock, 0, 0, 2).unwrap();
        assert_eq!(inventory.count, 4);
        assert!(!inventory.truncated);
        assert_eq!(inventory.report.visited_buses, 2);
        let mut found = inventory.iter();
        let _first = found.next().unwrap();
        let second = found.next().unwrap();
        let third = found.next().unwrap();
        let fourth = found.next().unwrap();
        assert_eq!(second.identity.address.function, 1);
        assert!(second.identity.is_vmware_svga_ii());
        assert_eq!(third.bridge.unwrap().secondary_bus, 2);
        assert!(fourth.identity.is_amd_display_candidate());
        assert_eq!(fourth.identity.address.bus, 2);
    }

    #[test]
    fn overflow_keeps_capacity_and_reports_truncation() {
        let mut mock = MockConfig::new(fixture);
        let inv = scan_topology::<2>(&mut mock, 0, 0, 2).unwrap();
        assert_eq!(inv.count, 2);
        assert!(inv.truncated);
        assert_eq!(inv.report.visited_buses, 2);
    }

    #[test]
    fn malformed_bridge_is_not_followed() {
        let mut mock = MockConfig::new(|a: PciAddress, o: u16| {
            if a.bus == 0 && a.device == 0 {
                Ok(match o {
                    0 => 0x0001_8086,
                    0x0c => 0x0001_0000,
                    0x08 => 0x0604_0000,
                    0x18 => 0x0001_0001, // primary 1, secondary 0: invalid
                    _ => u32::MAX,
                })
            } else { Ok(u32::MAX) }
        });
        let inv = scan_topology::<8>(&mut mock, 0, 0, 5).unwrap();
        assert_eq!(inv.report.malformed_bridges, 1);
        assert_eq!(inv.report.visited_buses, 1);
        assert_eq!(inv.count, 1);
    }

    #[test]
    fn bad_range_and_bad_headers_fail_closed() {
        let mut mock = MockConfig::new(|a: PciAddress, o: u16| {
            if a.bus == 0 && a.device == 3 {
                Ok(match o { 0 => 0x1234_1002, 0x0c => 0x007f_0000, _ => 0 })
            } else { Ok(u32::MAX) }
        });
        assert_eq!(scan_topology::<3>(&mut mock, 0, 10, 9), Err(ScanError::InvalidBusRange));
        let inv = scan_topology::<3>(&mut mock, 0, 0, 1).unwrap();
        assert_eq!(inv.report.malformed_headers, 1);
        assert_eq!(inv.count, 0);
    }

    #[test]
    fn empty_backend_is_harmless() {
        let mut mock = MockConfig::new(|_, _| Ok(u32::MAX));
        let inv = scan_topology::<0>(&mut mock, 0, 0, 255).unwrap();
        assert_eq!(inv.count, 0);
        assert!(!inv.truncated);
        assert_eq!(inv.report.visited_buses, 1);
    }
}
