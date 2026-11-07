//! Unified read-only PCI identity, topology and ACPI MCFG support.
//! The authoritative inventory is owned by the caller, never a second static scanner.
pub mod config;
pub mod ecam;
pub mod enumerate;
pub mod identity;
pub mod mcfg;

pub use config::{ConfigError, LegacyConfig, MockConfig, PciConfigRead};
pub use enumerate::{PciInventory, ScanError, ScanReport, scan_legacy_segment0, scan_topology};
pub use identity::{AddressError, PciAddress, PciBridge, PciClass, PciDeviceInfo, PciIdentity};
pub use mcfg::{McfgAllocation, McfgError, McfgTable, parse_mcfg};
