//! Stage 3: arithmetic-only PCI resource preflight for future MMIO.
//!
//! PCI BAR address registers do NOT encode aperture lengths. The Resource
//! lengths must come from a separate trusted PCI resource allocator, which
//! ZillaOS does not yet implement. Validation NEVER authorizes MMIO access.

use super::{capabilities::Snapshot, pci::PciDevice};

const PAGE_BYTES: u64 = 4096;
const MIN_LEGACY_FIFO_DATA: u64 = 10 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resource {
    pub physical_base: u64,
    pub length: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub framebuffer: Resource,
    pub fifo: Resource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    InvalidPhysicalWidth,
    InvalidResource,
    ResourceOutsidePhysicalAddressSpace,
    BarAddressMismatch,
    AperturesOverlap,
    MemoryDecodeDisabled,
    DeviceLengthExceedsAperture,
    UnsupportedFifo,
}

fn validate_resource(resource: Resource, physical_bits: u8) -> Result<u64, LayoutError> {
    if resource.length < PAGE_BYTES
        || !resource.length.is_power_of_two()
        || resource.physical_base == 0
        || resource.physical_base % resource.length != 0
    {
        return Err(LayoutError::InvalidResource);
    }
    let end = resource
        .physical_base
        .checked_add(resource.length)
        .ok_or(LayoutError::ResourceOutsidePhysicalAddressSpace)?;
    if end > (1u64 << physical_bits) {
        return Err(LayoutError::ResourceOutsidePhysicalAddressSpace);
    }
    Ok(end)
}

impl Layout {
    /// Arithmetic check using externally supplied PCI BAR resource lengths.
    /// Callers must first prove lengths using PCI resource allocation, never
    /// fabricate them from device size registers. No mapping is installed.
    pub fn validate(
        self,
        pci: PciDevice,
        device: Snapshot,
        physical_bits: u8,
    ) -> Result<(), LayoutError> {
        if !(32..=52).contains(&physical_bits) {
            return Err(LayoutError::InvalidPhysicalWidth);
        }
        if !pci.memory_decode_enabled {
            return Err(LayoutError::MemoryDecodeDisabled);
        }
        if self.framebuffer.physical_base != u64::from(pci.framebuffer_bar)
            || self.fifo.physical_base != u64::from(pci.fifo_bar)
        {
            return Err(LayoutError::BarAddressMismatch);
        }
        let fb_end = validate_resource(self.framebuffer, physical_bits)?;
        let fifo_end = validate_resource(self.fifo, physical_bits)?;
        if self.framebuffer.physical_base < fifo_end
            && self.fifo.physical_base < fb_end
        {
            return Err(LayoutError::AperturesOverlap);
        }
        let Some(fifo_bytes) = device.fifo_bytes else {
            return Err(LayoutError::UnsupportedFifo);
        };
        if u64::from(fifo_bytes) < MIN_LEGACY_FIFO_DATA + 16
            || u64::from(fifo_bytes) > self.fifo.length
        {
            return Err(LayoutError::DeviceLengthExceedsAperture);
        }
        let required_fb = u64::from(device.framebuffer_offset)
            .checked_add(u64::from(device.framebuffer_bytes))
            .ok_or(LayoutError::DeviceLengthExceedsAperture)?;
        if u64::from(device.vram_bytes) > self.framebuffer.length
            || required_fb > self.framebuffer.length
        {
            return Err(LayoutError::DeviceLengthExceedsAperture);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_pci::PciLocation;

    fn pci() -> PciDevice {
        PciDevice {
            location: PciLocation { bus: 0, device: 15, function: 0 },
            io_base: 0x1070,
            framebuffer_bar: 0xe000_0000,
            fifo_bar: 0xe800_0000,
            memory_decode_enabled: true,
        }
    }

    fn device() -> Snapshot {
        Snapshot {
            interface_id: 0x9000_0002,
            max_width: 1920,
            max_height: 1080,
            vram_bytes: 64 * 1024 * 1024,
            framebuffer_bytes: 32 * 1024 * 1024,
            framebuffer_offset: 0,
            fifo_bytes: Some(1024 * 1024),
            capability_bits: Some(0),
        }
    }

    fn layout() -> Layout {
        Layout {
            framebuffer: Resource {
                physical_base: 0xe000_0000,
                length: 128 * 1024 * 1024,
            },
            fifo: Resource {
                physical_base: 0xe800_0000,
                length: 1024 * 1024,
            },
        }
    }

    #[test]
    fn accepts_aligned_separate_resources_with_explicit_lengths() {
        assert_eq!(layout().validate(pci(), device(), 36), Ok(()));
    }

    #[test]
    fn rejects_untrusted_bar_address_and_disabled_decode() {
        let mut l = layout();
        l.fifo.physical_base += 0x1000;
        assert_eq!(l.validate(pci(), device(), 36), Err(LayoutError::BarAddressMismatch));
        let mut p = pci();
        p.memory_decode_enabled = false;
        assert_eq!(
            layout().validate(p, device(), 36),
            Err(LayoutError::MemoryDecodeDisabled)
        );
    }

    #[test]
    fn rejects_aperture_overlap_even_when_bases_differ() {
        let mut l = layout();
        l.framebuffer.length = 256 * 1024 * 1024;
        assert_eq!(
            l.validate(pci(), device(), 36),
            Err(LayoutError::AperturesOverlap)
        );
    }

    #[test]
    fn rejects_resource_alignment_and_physical_overflow() {
        let mut l = layout();
        l.fifo.length = 7000;
        assert_eq!(l.validate(pci(), device(), 36), Err(LayoutError::InvalidResource));
        assert_eq!(
            validate_resource(
                Resource { physical_base: 0x1_0000_0000, length: 4096 }, 32
            ),
            Err(LayoutError::ResourceOutsidePhysicalAddressSpace)
        );
        assert_eq!(layout().validate(pci(), device(), 0), Err(LayoutError::InvalidPhysicalWidth));
    }

    #[test]
    fn register_size_is_not_authority_for_aperture_length() {
        let mut l = layout();
        l.fifo.length = 4096;
        assert_eq!(
            l.validate(pci(), device(), 36),
            Err(LayoutError::DeviceLengthExceedsAperture)
        );
        let mut d = device();
        d.fifo_bytes = None;
        assert_eq!(
            layout().validate(pci(), d, 36),
            Err(LayoutError::UnsupportedFifo)
        );
    }

    #[test]
    fn rejects_framebuffer_offset_or_vram_larger_than_resource() {
        let mut d = device();
        d.framebuffer_offset = 128 * 1024 * 1024;
        assert_eq!(
            layout().validate(pci(), d, 36),
            Err(LayoutError::DeviceLengthExceedsAperture)
        );
        let mut d = device();
        d.vram_bytes = 256 * 1024 * 1024;
        assert_eq!(
            layout().validate(pci(), d, 36),
            Err(LayoutError::DeviceLengthExceedsAperture)
        );
    }
}
