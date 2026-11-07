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
struct HhdmResponse {
    revision: u64,
    offset: u64,
}

#[repr(C)]
struct StackSizeResponse {
    revision: u64,
}

#[repr(C)]
struct StackSizeRequest {
    id: [u64; 4],
    revision: u64,
    response: AtomicPtr<StackSizeResponse>,
    stack_size: u64,
}

/// Initial five fields of Limine's base-revision-6 limine_file.
/// Only these fields are accessed; protocol owns the rest of the struct.
#[repr(C)]
struct ModuleFile {
    revision: u64,
    address: *const u8,
    size: u64,
    path: *const u8,
    string: *const u8,
}

#[repr(C)]
struct ModuleResponse {
    revision: u64,
    module_count: u64,
    modules: *const *const ModuleFile,
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
static HHDM: Request<HhdmResponse> = Request {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x48dcf1cb8ad2b852,
        0x63984e959a98244b,
    ],
    revision: 0,
    response: AtomicPtr::new(ptr::null_mut()),
};

#[used]
#[unsafe(link_section = ".limine_requests")]
static STACK_SIZE: StackSizeRequest = StackSizeRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x224ef0460a8e8926,
        0xe1cb0fc25f46ea3d,
    ],
    revision: 0,
    response: AtomicPtr::new(ptr::null_mut()),
    stack_size: 256 * 1024,
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
#[unsafe(link_section = ".limine_requests")]
static MODULES: Request<ModuleResponse> = Request {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x3e7e279702be32af,
        0xca1c4f3bd1280cee,
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
    MissingHhdm,
    InvalidHhdm,
}


/// Limine's mapped higher-half offset, discovered dynamically per boot.
///
/// The direct map covers USABLE RAM on base revision >= 3. The returned
/// offset alone does not authorize dereferencing arbitrary physical ranges.
pub fn hhdm_offset() -> Result<u64, BootError> {
    let response = HHDM.response.load(Ordering::Acquire);
    if response.is_null() {
        return Err(BootError::MissingHhdm);
    }
    // SAFETY: trusted Limine response exists throughout early boot.
    let offset = unsafe { (*response).offset };
    // The HHDM must be a high-half mapped window, never an identity map.
    if offset < 0xff00_0000_0000_0000 || offset & 0xfff != 0 {
        return Err(BootError::InvalidHhdm);
    }
    Ok(offset)
}


/// Copy/inspect an optional, bounded image module synchronously.
/// The callback MUST consume the bytes immediately: bootloader module pages
/// may be reclaimed in the future. Only the module with the exact
/// "zillaos-wallpaper" string is accepted, never the first arbitrary module.
///
/// SAFETY: Limine base revision 6 provides valid mapped response structures,
/// pointers to module descriptors, and zero-terminated module strings while
/// the early kernel retains bootloader-owned pages. Count and data length are
/// bounded, and the bitmap parser independently validates every byte range.
pub fn with_wallpaper_module<R>(inspect: impl FnOnce(&[u8]) -> R) -> Option<R> {
    if BASE_REVISION.revision.load(Ordering::Acquire) != 0 {
        return None;
    }
    let response_ptr = MODULES.response.load(Ordering::Acquire);
    if response_ptr.is_null() { return None; }
    // SAFETY: Limine mapped response pointer, not reclaimed in early boot.
    let response = unsafe { &*response_ptr };
    let count = usize::try_from(response.module_count).ok()?;
    if count == 0 || count > 16 || response.modules.is_null() { return None; }
    for index in 0..count {
        // SAFETY: response contains exactly module_count descriptors.
        let file_ptr = unsafe { *response.modules.add(index) };
        if file_ptr.is_null() { continue; }
        // SAFETY: descriptor pointer is Limine-owned until boot reclamation.
        let file = unsafe { &*file_ptr };
        if file.string.is_null() || file.address.is_null() {
            continue;
        }
        const LABEL: &[u8] = b"zillaos-wallpaper\0";
        // SAFETY: Limine provides a zero-terminated ASCII module string;
        // stop comparing after the first short-circuit mismatch.
        let name_matches = LABEL.iter().enumerate().all(|(n, b)| {
            unsafe { *file.string.add(n) == *b }
        });
        if !name_matches { continue; }
        let length = usize::try_from(file.size).ok()?;
        if !(54..=zillaos_kernel::video::bmp::MAX_FILE_BYTES).contains(&length)
            || (file.address as usize).checked_add(length).is_none()
        {
            return None;
        }
        // SAFETY: file.address points to at least file.size mapped bytes.
        let bytes = unsafe { core::slice::from_raw_parts(file.address, length) };
        return Some(inspect(bytes));
    }
    None
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
