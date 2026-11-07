//! Platform PCI discovery and device ownership models.
//! All modules are no_std, bounded and hardware-passive at this stage.
pub mod device;
pub mod pci;
pub mod resources;
pub mod amd_bridge;
