//! Minimal audited Limine protocol structures for v0.0.1.
//! The canonical source is limine-protocol/PROTOCOL.md, base revision 6.
//! We copy a bounded memory map before any future reclaiming of boot data.

use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use zillaos_kernel::memory::{MemoryRegion, MAX_MEMORY_REGIONS};

#[repr(C)]
struct RevisionTag {
    magic1: u64,
    magic2: u64,
    revision: AtomicU64,
}

#[repr(C)]
struct Request<T> {
    id: [u64; 4],
    revision: u64,
    response: AtomicPtr<T>,
}

#[repr(C)]
struct MemoryMapResponse {
    revision: u64,
    entry_count: u64,
    entries: *const *const MemoryMapEntry,
}

#[repr(C)]
struct FramebufferResponse {
    revision: u64,
    framebuffer_count: u64,
    framebuffers: *const *const RawFramebuffer,
}

#[repr(C)]
struct RawFramebuffer {
    address: *mut u8,
    width: u64,
    height: u64,
    pitch: u64,
    bpp: u16,
    memory_model: u8,
    red_size: u8,
    red_shift: u8,
    green_size: u8,
    green_shift: u8,
    blue_size: u8,
    blue_shift: u8,
    unused: [u8; 7],
    edid_size: u64,
    edid: *const u8,
}

pub struct Framebuffer {
    pub address: *mut u8,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
}

#[repr(C)]
#[allow(dead_code)]
struct MemoryMapEntry {
    base: u64,
    length: u64,
    kind: u64,
}

#[used]
#[unsafe(link_section = ".limine_requests_start")]
static REQUESTS_START: [u64; 4] = [
    0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf,
    0x785c6ed015d3e316, 0x181e920a7852b9d9,
];

#[used]
#[unsafe(link_section = ".limine_requests")]
static BASE_REVISION: RevisionTag = RevisionTag {
    magic1: 0xf9562b2d5c95a6c8,
    magic2: 0x6a7b384944536bdc,
    revision: AtomicU64::new(6),
};

#[used]
#[unsafe(link_section = ".limine_requests")]
static MEMORY_MAP: Request<MemoryMapResponse> = Request {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x67cf3d9d378a806f,
        0xe304acdfc50c3c62,
    ],
    revision: 0,
    response: AtomicPtr::new(ptr::null_mut()),
};

#[used]
#[unsafe(link_section = ".limine_requests")]
static FRAMEBUFFER: Request<FramebufferResponse> = Request {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x9d5827dcd881dd75,
        0xa3148604f6fab11b,
    ],
    revision: 0,
    response: AtomicPtr::new(ptr::null_mut()),
};

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static REQUESTS_END: [u64; 2] = [
    0xadc0e0531bb10d03, 0x9572709f31764c62,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootError {
    UnsupportedRevision,
    MissingMemoryMap,
    InvalidMemoryMap,
}

pub struct BootMemoryMap {
    pub regions: [MemoryRegion; MAX_MEMORY_REGIONS],
    pub count: usize,
}

/// Copy the bootloader memory map while the Limine boot-memory region is alive.
/// Later stages must not retain pointers into Limine's reclaimable structures.
pub fn memory_map() -> Result<BootMemoryMap, BootError> {
    if BASE_REVISION.revision.load(Ordering::Relaxed) != 0 {
        return Err(BootError::UnsupportedRevision);
    }
    let response = MEMORY_MAP.response.load(Ordering::Acquire);
    if response.is_null() {
        return Err(BootError::MissingMemoryMap);
    }
    // SAFETY: trusted bootloader response is valid during initial handoff;
    // no reclamation, remapping, or concurrency is possible at this point.
    let response = unsafe { &*response };
    let count = usize::try_from(response.entry_count).map_err(|_| BootError::InvalidMemoryMap)?;
    if count == 0 || count > MAX_MEMORY_REGIONS || response.entries.is_null() {
        return Err(BootError::InvalidMemoryMap);
    }
    let mut regions = [MemoryRegion::EMPTY; MAX_MEMORY_REGIONS];
    for (index, slot) in regions.iter_mut().take(count).enumerate() {
        // SAFETY: Limine guarantees the pointer array has entry_count entries;
        // count was bounded above and NULL elements are separately rejected.
        let entry = unsafe { *response.entries.add(index) };
        if entry.is_null() {
            return Err(BootError::InvalidMemoryMap);
        }
        // SAFETY: this entry is bootloader-owned and valid until reclamation.
        // Copy scalar fields immediately; do not expose a bootloader pointer.
        let entry = unsafe { &*entry };
        *slot = MemoryRegion {
            base: entry.base,
            length: entry.length,
            kind: entry.kind,
        };
    }
    Ok(BootMemoryMap { regions, count })
}

/** A strictly bounded 32-bit RGB framebuffer for the QEMU boot splash.

The pointers originate from the already-trusted Limine bootloader. We do not
claim native hardware verification or arbitrary firmware-pointer safety.
Returned pixel bounds are capped to 64 MiB before rendering.
*/
pub fn framebuffer() -> Option<Framebuffer> {
    let response = FRAMEBUFFER.response.load(Ordering::Acquire);
    if response.is_null() {
        return None;
    }
    // SAFETY: bootloader-provided response during the boot handoff.
    let response = unsafe { &*response };
    if response.framebuffer_count == 0 || response.framebuffers.is_null() {
        return None;
    }
    // SAFETY: Limine guarantees at least framebuffer_count valid pointers.
    let first = unsafe { *response.framebuffers };
    if first.is_null() {
        return None;
    }
    // SAFETY: Limine provides a mapped framebuffer structure.
    let fb = unsafe { &*first };
    let width = usize::try_from(fb.width).ok()?;
    let height = usize::try_from(fb.height).ok()?;
    let pitch = usize::try_from(fb.pitch).ok()?;
    if fb.address.is_null()
        || (fb.address as usize) % 4 != 0
        || fb.bpp != 32
        || fb.memory_model != 1
        || fb.red_size != 8
        || fb.green_size != 8
        || fb.blue_size != 8
        || fb.red_shift > 24
        || fb.green_shift > 24
        || fb.blue_shift > 24
        || width < 320
        || height < 200
        || width > 8192
        || height > 8192
        || pitch % 4 != 0
        || pitch < width.checked_mul(4)?
        || height.checked_mul(pitch)? > 64 * 1024 * 1024
    {
        return None;
    }
    Some(Framebuffer {
        address: fb.address,
        width,
        height,
        pitch,
        red_shift: fb.red_shift,
        green_shift: fb.green_shift,
        blue_shift: fb.blue_shift,
    })
}
