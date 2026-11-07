//! Unified read-only PCI identity, topology and ACPI MCFG support.
//! The authoritative inventory is owned by the caller, never a second static scanner.
pub mod identity;
pub mod config;
pub mod enumerate;
pub mod mcfg;
pub mod ecam;

pub use config::{ConfigError, LegacyConfig, MockConfig, PciConfigRead};
pub use enumerate::{PciInventory, ScanError, ScanReport, scan_topology};
pub use identity::{AddressError, PciAddress, PciBridge, PciClass, PciDeviceInfo, PciIdentity};
pub use mcfg::{McfgAllocation, McfgError, McfgTable, parse_mcfg};
