//! Minimal audited Limine protocol structures for v0.0.1.
//! The canonical source is limine-protocol/PROTOCOL.md, base revision 6.
//! No allocator is available yet. We only read the memory-map count.

use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

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

/// The bootloader owns request responses. These pointers must never be
/// interpreted as coming from arbitrary user memory. They are only available
/// during boot, and must be copied before reclaiming bootloader memory.
pub fn memory_region_count() -> Result<u64, BootError> {
    if BASE_REVISION.revision.load(Ordering::Relaxed) != 0 {
        return Err(BootError::UnsupportedRevision);
    }
    let response = MEMORY_MAP.response.load(Ordering::Acquire);
    if response.is_null() {
        return Err(BootError::MissingMemoryMap);
    }
    // SAFETY: after successful Limine handoff, the response pointer references
    // a bootloader-owned mapped MemoryMapResponse. We read only fixed-size
    // fields here, and perform no dereference of individual memory entries.
    let response = unsafe { &*response };
    if response.entry_count == 0
        || response.entry_count > 4096
        || response.entries.is_null()
    {
        return Err(BootError::InvalidMemoryMap);
    }
    Ok(response.entry_count)
}
