//! Hardware-independent display object model. No MMIO, allocation or IPC.
//!
//! These bounded registries are mockable lifecycle contracts, NOT kernel
//! capability tables: future IPC must authenticate ownership and rights.

use super::display_manager::{BackendFailure, Mode, ModeBackend};

pub const MAX_SURFACE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputId {
    pub slot: u16,
    pub generation: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceId {
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// Little-endian bytes B, G, R, unused; no alpha blending.
    Xrgb8888,
    /// Little-endian bytes B, G, R, A.
    Argb8888,
    /// Packed little-endian RGB 5:6:5. Not a claim of driver support.
    Rgb565,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Xrgb8888 | Self::Argb8888 => 4,
            Self::Rgb565 => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceOwner {
    Kernel,
    Process(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferLayout {
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
    pub format: PixelFormat,
    pub byte_length: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceError {
    InvalidOwner,
    InvalidDimensions,
    InvalidStride,
    SizeOverflow,
    BufferTooSmall,
    BudgetExceeded,
    InvalidDamage,
    NoSlots,
    StaleHandle,
    WrongOwner,
    ScanoutBusy,
    NotInScanout,
}

impl BufferLayout {
    /// Require the entire pitched allocation, including trailing row padding.
    /// A BAR length, mapped virtual pointer, and GPU compatibility must be
    /// verified separately by the device resource broker.
    pub fn validate(self) -> Result<u64, SurfaceError> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
        {
            return Err(SurfaceError::InvalidDimensions);
        }
        let bpp = u64::from(self.format.bytes_per_pixel());
        let row_bytes = u64::from(self.width)
            .checked_mul(bpp)
            .ok_or(SurfaceError::SizeOverflow)?;
        if u64::from(self.pitch_bytes) < row_bytes
            || u64::from(self.pitch_bytes) % bpp != 0
        {
            return Err(SurfaceError::InvalidStride);
        }
        let required = u64::from(self.pitch_bytes)
            .checked_mul(u64::from(self.height))
            .ok_or(SurfaceError::SizeOverflow)?;
        if required > MAX_SURFACE_BYTES {
            return Err(SurfaceError::BudgetExceeded);
        }
        if self.byte_length < required {
            return Err(SurfaceError::BufferTooSmall);
        }
        Ok(required)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DamageRect {
    pub fn validate(self, layout: BufferLayout) -> Result<(), SurfaceError> {
        layout.validate()?;
        if self.width == 0 || self.height == 0
            || self.x.checked_add(self.width).is_none_or(|end| end > layout.width)
            || self.y.checked_add(self.height).is_none_or(|end| end > layout.height)
        {
            return Err(SurfaceError::InvalidDamage);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDescriptor {
    pub owner: SurfaceOwner,
    pub layout: BufferLayout,
}

impl SurfaceDescriptor {
    pub fn validate(self) -> Result<(), SurfaceError> {
        if matches!(self.owner, SurfaceOwner::Process(0)) {
            return Err(SurfaceError::InvalidOwner);
        }
        self.layout.validate().map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferOwnership {
    CpuWritable,
    ScanoutReadOnly,
}

#[derive(Clone, Copy)]
struct SurfaceSlot {
    generation: u32,
    descriptor: Option<SurfaceDescriptor>,
    ownership: BufferOwnership,
    retired: bool,
}

impl SurfaceSlot {
    const EMPTY: Self = Self {
        generation: 1,
        descriptor: None,
        ownership: BufferOwnership::CpuWritable,
        retired: false,
    };
}

/// Fixed-capacity model; never maps or frees real memory. Slots are retired
/// rather than reusing a wrapped generation, preventing handle resurrection.
pub struct SurfaceRegistry<const N: usize> {
    slots: [SurfaceSlot; N],
}

impl<const N: usize> SurfaceRegistry<N> {
    pub const fn new() -> Self {
        Self { slots: [SurfaceSlot::EMPTY; N] }
    }

    pub fn create(&mut self, descriptor: SurfaceDescriptor) -> Result<SurfaceId, SurfaceError> {
        descriptor.validate()?;
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.descriptor.is_none() && !slot.retired {
                let slot_number = u32::try_from(index).map_err(|_| SurfaceError::NoSlots)?;
                slot.descriptor = Some(descriptor);
                slot.ownership = BufferOwnership::CpuWritable;
                return Ok(SurfaceId { slot: slot_number, generation: slot.generation });
            }
        }
        Err(SurfaceError::NoSlots)
    }

    fn slot_mut(&mut self, id: SurfaceId) -> Result<&mut SurfaceSlot, SurfaceError> {
        let slot = self.slots.get_mut(id.slot as usize).ok_or(SurfaceError::StaleHandle)?;
        if id.generation == 0 || slot.generation != id.generation || slot.descriptor.is_none() {
            return Err(SurfaceError::StaleHandle);
        }
        Ok(slot)
    }

    pub fn descriptor(&self, id: SurfaceId) -> Result<SurfaceDescriptor, SurfaceError> {
        let slot = self.slots.get(id.slot as usize).ok_or(SurfaceError::StaleHandle)?;
        if id.generation == 0 || slot.generation != id.generation {
            return Err(SurfaceError::StaleHandle);
        }
        slot.descriptor.ok_or(SurfaceError::StaleHandle)
    }

    pub fn submit_damage(
        &mut self,
        id: SurfaceId,
        requester: SurfaceOwner,
        rect: DamageRect,
    ) -> Result<(), SurfaceError> {
        let slot = self.slot_mut(id)?;
        let descriptor = slot.descriptor.ok_or(SurfaceError::StaleHandle)?;
        if descriptor.owner != requester {
            return Err(SurfaceError::WrongOwner);
        }
        if slot.ownership != BufferOwnership::CpuWritable {
            return Err(SurfaceError::ScanoutBusy);
        }
        rect.validate(descriptor.layout)
    }

    /// Models an exclusive scanout lease: CPU writes and destruction are
    /// forbidden until the future display fence has actually retired.
    pub fn acquire_scanout(&mut self, id: SurfaceId) -> Result<(), SurfaceError> {
        let slot = self.slot_mut(id)?;
        if slot.ownership == BufferOwnership::ScanoutReadOnly {
            return Err(SurfaceError::ScanoutBusy);
        }
        slot.ownership = BufferOwnership::ScanoutReadOnly;
        Ok(())
    }

    /// Only the trusted display owner may call this after fence retirement.
    pub fn release_scanout(&mut self, id: SurfaceId) -> Result<(), SurfaceError> {
        let slot = self.slot_mut(id)?;
        if slot.ownership != BufferOwnership::ScanoutReadOnly {
            return Err(SurfaceError::NotInScanout);
        }
        slot.ownership = BufferOwnership::CpuWritable;
        Ok(())
    }

    pub fn destroy(&mut self, id: SurfaceId, requester: SurfaceOwner) -> Result<(), SurfaceError> {
        let slot = self.slot_mut(id)?;
        if slot.descriptor.ok_or(SurfaceError::StaleHandle)?.owner != requester {
            return Err(SurfaceError::WrongOwner);
        }
        if slot.ownership == BufferOwnership::ScanoutReadOnly {
            return Err(SurfaceError::ScanoutBusy);
        }
        slot.descriptor = None;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
        } else {
            slot.retired = true;
        }
        Ok(())
    }
}

impl<const N: usize> Default for SurfaceRegistry<N> {
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Copy)]
struct OutputSlot {
    generation: u16,
    connected: bool,
    retired: bool,
}

impl OutputSlot {
    const EMPTY: Self = Self { generation: 1, connected: false, retired: false };
}

/// Logical output identity lifecycle, independent of EDID or GPU topology.
pub struct OutputRegistry<const N: usize> {
    slots: [OutputSlot; N],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputError {
    InvalidSlot,
    AlreadyConnected,
    StaleHandle,
    Retired,
}

impl<const N: usize> OutputRegistry<N> {
    pub const fn new() -> Self { Self { slots: [OutputSlot::EMPTY; N] } }

    pub fn connect(&mut self, index: usize) -> Result<OutputId, OutputError> {
        let slot = self.slots.get_mut(index).ok_or(OutputError::InvalidSlot)?;
        let index = u16::try_from(index).map_err(|_| OutputError::InvalidSlot)?;
        if slot.retired { return Err(OutputError::Retired); }
        if slot.connected { return Err(OutputError::AlreadyConnected); }
        slot.connected = true;
        Ok(OutputId { slot: index, generation: slot.generation })
    }

    pub fn is_connected(&self, id: OutputId) -> bool {
        id.generation != 0 && self.slots.get(usize::from(id.slot))
            .is_some_and(|s| s.connected && s.generation == id.generation)
    }

    pub fn disconnect(&mut self, id: OutputId) -> Result<(), OutputError> {
        if !self.is_connected(id) { return Err(OutputError::StaleHandle); }
        let slot = &mut self.slots[usize::from(id.slot)];
        slot.connected = false;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
        } else {
            slot.retired = true;
        }
        Ok(())
    }
}

impl<const N: usize> Default for OutputRegistry<N> {
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeCapability {
    pub mode: Mode,
    /// Unknown is not 60 Hz. A driver supplies this only from evidence.
    pub refresh_millihz: Option<u32>,
}

/// Future output-facing extension of the existing single-output ModeBackend.
/// AMD and VMware adapters implement this; DisplayManager remains the only
/// transaction owner. No driver implementation is wired to hardware yet.
pub trait VideoDisplayBackend: ModeBackend {
    fn driver_name(&self) -> &'static str;
    fn active_output(&self) -> Option<OutputId>;
    /// The caller provides bounded storage; returned count must not exceed
    /// out.len(). Entries are validated driver evidence, not guessed modes.
    fn enumerate_modes(
        &self,
        output: OutputId,
        out: &mut [Option<ModeCapability>],
    ) -> Result<usize, BackendFailure>;
    /// Test only: must never write hardware. Real drivers require validated
    /// ownership, scanout format, aperture and connector state first.
    fn test_scanout(
        &mut self,
        output: OutputId,
        surface: SurfaceDescriptor,
    ) -> Result<(), BackendFailure>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeListError {
    StaleOutput,
    Backend(BackendFailure),
    TooManyModes,
    MissingMode,
    InvalidMode,
    DuplicateMode,
}

/// Validate a bounded, device-provided mode catalogue. Output generation,
/// mode geometry, refresh metadata and duplicates are checked. This does not
/// infer modes from PCI register maxima and cannot verify physical displays.
pub fn discover_modes(
    backend: &impl VideoDisplayBackend,
    output: OutputId,
    out: &mut [Option<ModeCapability>],
) -> Result<usize, ModeListError> {
    if backend.active_output() != Some(output) || output.generation == 0 {
        return Err(ModeListError::StaleOutput);
    }
    out.fill(None);
    let count = backend.enumerate_modes(output, out).map_err(ModeListError::Backend)?;
    if count > out.len() {
        return Err(ModeListError::TooManyModes);
    }
    for i in 0..count {
        let entry = out[i].ok_or(ModeListError::MissingMode)?;
        if !entry.mode.is_sane() || entry.refresh_millihz == Some(0) {
            return Err(ModeListError::InvalidMode);
        }
        for earlier in out.iter().take(i) {
            if earlier.is_some_and(|m| m == entry) {
                return Err(ModeListError::DuplicateMode);
            }
        }
    }
    // A broken driver must not smuggle unchecked modes outside count.
    if out.iter().skip(count).any(Option::is_some) {
        return Err(ModeListError::TooManyModes);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> SurfaceDescriptor {
        SurfaceDescriptor {
            owner: SurfaceOwner::Process(42),
            layout: BufferLayout {
                width: 640, height: 480, pitch_bytes: 2560,
                format: PixelFormat::Xrgb8888,
                byte_length: 2560 * 480,
            },
        }
    }

    #[test]
    fn formats_and_layout_bounds() {
        assert_eq!(PixelFormat::Xrgb8888.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Argb8888.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Rgb565.bytes_per_pixel(), 2);
        assert_eq!(descriptor().layout.validate(), Ok(1_228_800));
        let mut d = descriptor();
        d.layout.format = PixelFormat::Rgb565;
        d.layout.pitch_bytes = 1280;
        d.layout.byte_length = 1280 * 480;
        assert_eq!(d.layout.validate(), Ok(614_400));
        d.layout.pitch_bytes = 1279;
        assert_eq!(d.layout.validate(), Err(SurfaceError::InvalidStride));
        d.layout.pitch_bytes = 1280;
        d.layout.byte_length = 1;
        assert_eq!(d.layout.validate(), Err(SurfaceError::BufferTooSmall));
        d.layout.width = 8193;
        assert_eq!(d.layout.validate(), Err(SurfaceError::InvalidDimensions));
        d.layout.width = 8192;
        d.layout.height = 8192;
        d.layout.pitch_bytes = 8192 * 4;
        d.layout.byte_length = u64::MAX;
        assert_eq!(d.layout.validate(), Ok(256 * 1024 * 1024));
        d.layout.pitch_bytes += 4;
        assert_eq!(d.layout.validate(), Err(SurfaceError::BudgetExceeded));
    }

    #[test]
    fn checked_damage_rectangles() {
        let layout = descriptor().layout;
        assert_eq!(DamageRect { x: 639, y: 479, width: 1, height: 1 }.validate(layout), Ok(()));
        for r in [
            DamageRect { x: 640, y: 0, width: 1, height: 1 },
            DamageRect { x: 0, y: 480, width: 1, height: 1 },
            DamageRect { x: u32::MAX, y: 0, width: 2, height: 1 },
            DamageRect { x: 0, y: 0, width: 0, height: 10 },
        ] {
            assert_eq!(r.validate(layout), Err(SurfaceError::InvalidDamage));
        }
    }

    #[test]
    fn surface_lease_owner_and_stale_generation() {
        let mut surfaces = SurfaceRegistry::<1>::new();
        let id = surfaces.create(descriptor()).unwrap();
        assert_eq!(surfaces.create(descriptor()), Err(SurfaceError::NoSlots));
        assert_eq!(surfaces.submit_damage(id, SurfaceOwner::Kernel,
            DamageRect { x: 0, y: 0, width: 1, height: 1 }), Err(SurfaceError::WrongOwner));
        assert_eq!(surfaces.acquire_scanout(id), Ok(()));
        assert_eq!(surfaces.destroy(id, SurfaceOwner::Process(42)), Err(SurfaceError::ScanoutBusy));
        assert_eq!(surfaces.submit_damage(id, SurfaceOwner::Process(42),
            DamageRect { x: 0, y: 0, width: 1, height: 1 }), Err(SurfaceError::ScanoutBusy));
        assert_eq!(surfaces.release_scanout(id), Ok(()));
        assert_eq!(surfaces.release_scanout(id), Err(SurfaceError::NotInScanout));
        assert_eq!(surfaces.destroy(id, SurfaceOwner::Process(42)), Ok(()));
        assert_eq!(surfaces.descriptor(id), Err(SurfaceError::StaleHandle));
        let next = surfaces.create(descriptor()).unwrap();
        assert_eq!(next.slot, id.slot);
        assert_ne!(next.generation, id.generation);
        assert_eq!(surfaces.acquire_scanout(id), Err(SurfaceError::StaleHandle));
    }

    #[test]
    fn output_disconnect_invalidates_generation() {
        let mut outputs = OutputRegistry::<1>::new();
        let first = outputs.connect(0).unwrap();
        assert!(outputs.is_connected(first));
        assert_eq!(outputs.connect(0), Err(OutputError::AlreadyConnected));
        assert_eq!(outputs.disconnect(first), Ok(()));
        assert!(!outputs.is_connected(first));
        assert_eq!(outputs.disconnect(first), Err(OutputError::StaleHandle));
        let next = outputs.connect(0).unwrap();
        assert_ne!(next.generation, first.generation);
        assert!(!outputs.is_connected(first));
    }

    struct MockModes {
        output: OutputId,
        list: [Option<ModeCapability>; 2],
        report: usize,
    }

    impl ModeBackend for MockModes {
        fn runtime_switch_available(&self) -> bool { false }
        fn supports_mode(&self, _: Mode) -> bool { false }
        fn prepare_mode(&mut self, _: Mode) -> Result<(), BackendFailure> {
            Err(BackendFailure::Unavailable)
        }
        fn set_mode(&mut self, _: Mode) -> Result<(), BackendFailure> {
            Err(BackendFailure::Unavailable)
        }
        fn read_mode(&mut self) -> Result<Mode, BackendFailure> {
            Err(BackendFailure::Unavailable)
        }
        fn can_restore(&self, _: Mode) -> bool { false }
    }

    impl VideoDisplayBackend for MockModes {
        fn driver_name(&self) -> &'static str { "mock-no-hardware" }
        fn active_output(&self) -> Option<OutputId> { Some(self.output) }
        fn enumerate_modes(
            &self, _: OutputId, out: &mut [Option<ModeCapability>],
        ) -> Result<usize, BackendFailure> {
            for (dst, src) in out.iter_mut().zip(self.list.iter()) {
                *dst = *src;
            }
            Ok(self.report)
        }
        fn test_scanout(&mut self, _: OutputId, _: SurfaceDescriptor)
            -> Result<(), BackendFailure>
        {
            Err(BackendFailure::Unavailable)
        }
    }

    #[test]
    fn mode_discovery_checks_output_generation_duplicates_and_counts() {
        let output = OutputId { slot: 0, generation: 3 };
        let a = ModeCapability { mode: Mode::new(1024, 768), refresh_millihz: None };
        let b = ModeCapability { mode: Mode::new(1280, 800), refresh_millihz: Some(60_000) };
        let mut backend = MockModes { output, list: [Some(a), Some(b)], report: 2 };
        let mut found = [None; 2];
        assert_eq!(discover_modes(&backend, output, &mut found), Ok(2));
        assert_eq!(found, [Some(a), Some(b)]);
        assert_eq!(discover_modes(&backend,
            OutputId { slot: 0, generation: 2 }, &mut found), Err(ModeListError::StaleOutput));
        backend.list = [Some(a), Some(a)];
        assert_eq!(discover_modes(&backend, output, &mut found), Err(ModeListError::DuplicateMode));
        backend.list = [Some(a), Some(b)];
        backend.report = 3;
        assert_eq!(discover_modes(&backend, output, &mut found), Err(ModeListError::TooManyModes));
        backend.report = 1;
        assert_eq!(discover_modes(&backend, output, &mut found), Err(ModeListError::TooManyModes));
        backend.list = [Some(ModeCapability { mode: a.mode, refresh_millihz: Some(0) }), None];
        assert_eq!(discover_modes(&backend, output, &mut found), Err(ModeListError::InvalidMode));
        backend.list = [None, None];
        assert_eq!(discover_modes(&backend, output, &mut found), Err(ModeListError::MissingMode));
    }

    #[test]
    fn rejects_unauthenticated_process_zero() {
        let mut d = descriptor();
        d.owner = SurfaceOwner::Process(0);
        assert_eq!(SurfaceRegistry::<2>::new().create(d), Err(SurfaceError::InvalidOwner));
    }
}
