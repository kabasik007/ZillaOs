//! Passive identification of the graphics adapter visible to an x86 PCI guest.
//! No device registers, BARs, memory mappings or FIFO are modified.
//! The framebuffer remains the Limine/UEFI mode; accelerated draw and
//! runtime resolution changes are NOT implemented.
use crate::storage_pci::PciLocation;
 
// Hardware-specific code is separate from raster, Desktop and WindowManager.
#[path = "../drivers/gpu/mod.rs"]
pub mod gpu;

// Pure transactional mode switching; no runtime hardware backend yet.
pub mod display_manager;

// Read-only bounded BMP asset decoder; no graphics hardware access.
pub mod bmp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoAdapter {
    FirmwareOnly,
    VmwareSvgaII,
    GenericPciDisplay,
}
impl VideoAdapter {
    pub const fn name(self) -> &'static str {
        match self {
            Self::FirmwareOnly => "UEFI GOP FRAMEBUFFER",
            Self::VmwareSvgaII => "VMWARE SVGA II (PCI)",
            Self::GenericPciDisplay => "PCI VIDEO (NO DRIVER)",
        }
    }
    pub const fn hardware_acceleration(self) -> bool { false }
    pub const fn runtime_mode_setting(self) -> bool { false }
}

pub fn probe_pci_video(mut config: impl FnMut(PciLocation, u8) -> u32) -> VideoAdapter {
    let mut other = false;
    for bus in 0..=255u8 {
        for device in 0..32u8 {
            let loc = PciLocation { bus, device, function: 0 };
            let id = config(loc, 0x00);
            let vendor = id as u16;
            if vendor == 0xffff || vendor == 0 { continue; }
            let class = config(loc, 0x08);
            if ((class >> 24) as u8) != 0x03 { continue; }
            if vendor == 0x15ad && (id >> 16) as u16 == 0x0405 {
                return VideoAdapter::VmwareSvgaII;
            }
            other = true;
        }
    }
    if other { VideoAdapter::GenericPciDisplay } else { VideoAdapter::FirmwareOnly }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_vmware_svga_ii_from_pci_display_class_only() {
        assert_eq!(probe_pci_video(|loc, off| {
            if loc.bus == 0 && loc.device == 15 {
                if off == 0 { 0x0405_15ad } else { 0x0300_0000 }
            } else { 0xffff_ffff }
        }), VideoAdapter::VmwareSvgaII);
    }
    #[test]
    fn does_not_mislabel_storage_as_accelerated_graphics() {
        assert_eq!(probe_pci_video(|loc, off| {
            if loc.device == 1 && loc.bus == 0 {
                if off == 0 { 0x0405_15ad } else { 0x0106_0000 }
            } else { 0xffff_ffff }
        }), VideoAdapter::FirmwareOnly);
        assert!(!VideoAdapter::VmwareSvgaII.hardware_acceleration());
        assert!(!VideoAdapter::VmwareSvgaII.runtime_mode_setting());
    }
    #[test]
    fn generic_video_does_not_claim_vmware_capabilities() {
        assert_eq!(probe_pci_video(|loc, off| {
            if loc.bus == 0 && loc.device == 2 {
                if off == 0 { 0x1111_1234 } else { 0x0300_0000 }
            } else { 0xffff_ffff }
        }), VideoAdapter::GenericPciDisplay);
    }
}
