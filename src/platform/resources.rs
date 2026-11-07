//! Kernel-only resource models and fail-closed hardware service contracts.
//! No BAR sizing, MMIO mapping, IRQ enabling, bus mastering or DMA occurs here.
use super::device::DeviceId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind { Mmio, IoPort, Irq }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceSource {
    /// Supplied by a future trusted PCI/firmware resource allocator.
    FirmwareValidated,
    /// Unverified PCI BAR read-back is NOT a verified resource extent.
    Unverified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceRange {
    pub kind: ResourceKind,
    pub base: u64,
    pub length: u64,
    pub source: ResourceSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceError {
    Empty,
    Overflow,
    NotVerified,
    NotEnabled,
    Unaligned,
    InvalidPolicy,
}

impl ResourceRange {
    pub const fn new(
        kind: ResourceKind, base: u64, length: u64, source: ResourceSource
    ) -> Result<Self, ResourceError> {
        if length == 0 { return Err(ResourceError::Empty); }
        if base.checked_add(length).is_none() { return Err(ResourceError::Overflow); }
        Ok(Self { kind, base, length, source })
    }

    pub const fn valid_for_claim(self) -> Result<(), ResourceError> {
        if self.length == 0 { return Err(ResourceError::Empty); }
        if self.base.checked_add(self.length).is_none() { return Err(ResourceError::Overflow); }
        if !matches!(self.source, ResourceSource::FirmwareValidated) {
            return Err(ResourceError::NotVerified);
        }
        Ok(())
    }

    pub const fn overlaps(self, other: Self) -> bool {
        if self.kind as u8 != other.kind as u8 { return false; }
        let Some(end) = self.base.checked_add(self.length) else { return true; };
        let Some(other_end) = other.base.checked_add(other.length) else { return true; };
        self.base < other_end && other.base < end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachePolicy { Uncached, WriteCombining }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmioMapping {
    pub device: DeviceId,
    pub resource: ResourceRange,
    pub cache: CachePolicy,
}

impl MmioMapping {
    pub fn preflight(self) -> Result<(), ResourceError> {
        self.resource.valid_for_claim()?;
        if !matches!(self.resource.kind, ResourceKind::Mmio) { return Err(ResourceError::InvalidPolicy); }
        if self.resource.base & 4095 != 0 || self.resource.length & 4095 != 0 {
            return Err(ResourceError::Unaligned);
        }
        Ok(())
    }
}

/// Deliberately has no implementation that can touch hardware.
/// Future implementations require kernel memory-map + cache alias validation.
pub trait MmioMapper {
    type Handle;
    fn map(&mut self, request: MmioMapping) -> Result<Self::Handle, ResourceError>;
    fn unmap(&mut self, handle: Self::Handle) -> Result<(), ResourceError>;
}

pub trait IrqAllocator {
    type Handle;
    fn reserve(&mut self, device: DeviceId, vector: u32) -> Result<Self::Handle, ResourceError>;
    fn release(&mut self, handle: Self::Handle) -> Result<(), ResourceError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaDirection { ToDevice, FromDevice, Bidirectional }

/// DMA CPU-physical and bus/IOVA addresses must never be assumed identical.
/// No bus mastering or IOMMU domains are created by these declarations.
pub trait DmaIommu {
    type Handle;
    fn map_pinned(&mut self, device: DeviceId, physical: u64, length: u64, direction: DmaDirection)
        -> Result<Self::Handle, ResourceError>;
    fn unmap(&mut self, handle: Self::Handle) -> Result<(), ResourceError>;
}

/// Default hardware backend for the current stage: all operations disabled.
pub struct DisabledHardware;

impl MmioMapper for DisabledHardware {
    type Handle = ();
    fn map(&mut self, _: MmioMapping) -> Result<(), ResourceError> {
        Err(ResourceError::NotEnabled)
    }
    fn unmap(&mut self, _: ()) -> Result<(), ResourceError> {
        Err(ResourceError::NotEnabled)
    }
}

impl IrqAllocator for DisabledHardware {
    type Handle = ();
    fn reserve(&mut self, _: DeviceId, _: u32) -> Result<(), ResourceError> {
        Err(ResourceError::NotEnabled)
    }
    fn release(&mut self, _: ()) -> Result<(), ResourceError> {
        Err(ResourceError::NotEnabled)
    }
}

impl DmaIommu for DisabledHardware {
    type Handle = ();
    fn map_pinned(&mut self, _: DeviceId, _: u64, _: u64, _: DmaDirection)
        -> Result<(), ResourceError> {
        Err(ResourceError::NotEnabled)
    }
    fn unmap(&mut self, _: ()) -> Result<(), ResourceError> {
        Err(ResourceError::NotEnabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unverified_or_overflowed_resources_are_rejected() {
        assert_eq!(ResourceRange::new(ResourceKind::Mmio, u64::MAX, 8, ResourceSource::FirmwareValidated), Err(ResourceError::Overflow));
        let r = ResourceRange::new(ResourceKind::Mmio, 0x1000, 0x1000, ResourceSource::Unverified).unwrap();
        assert_eq!(r.valid_for_claim(), Err(ResourceError::NotVerified));
        let id = DeviceId { slot: 0, generation: 1 };
        assert_eq!(MmioMapping { device: id, resource: r, cache: CachePolicy::Uncached }.preflight(), Err(ResourceError::NotVerified));
    }
    #[test]
    fn exclusive_ranges_use_half_open_intervals() {
        let r = |base, length| ResourceRange::new(ResourceKind::Mmio, base, length, ResourceSource::FirmwareValidated).unwrap();
        assert!(!r(0x1000,0x1000).overlaps(r(0x2000,0x1000)));
        assert!(r(0x1000,0x1000).overlaps(r(0x1fff,0x1000)));
    }
    #[test]
    fn disabled_hardware_rejects_every_operation() {
        let mut hw = DisabledHardware;
        let id = DeviceId { slot: 0, generation: 1 };
        let r = ResourceRange::new(ResourceKind::Mmio, 0x1000, 0x1000, ResourceSource::FirmwareValidated).unwrap();
        assert_eq!(hw.map(MmioMapping { device: id, resource: r, cache: CachePolicy::Uncached }), Err(ResourceError::NotEnabled));
        assert_eq!(hw.reserve(id, 50), Err(ResourceError::NotEnabled));
        assert_eq!(hw.map_pinned(id, 0x2000, 4096, DmaDirection::ToDevice), Err(ResourceError::NotEnabled));
    }
}
