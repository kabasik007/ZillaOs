//! Early boot BMP wallpaper lifetime: copy decoded pixels out of Limine
//! modules before any possible future bootloader-memory reclamation.
//! The image is immutable after a single startup initialization, and only
//! one bootstrap CPU owns the early GUI renderer (no compositor SMP yet).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use zillaos_kernel::video::bmp::{self, ImageInfo, MAX_PIXELS};

use crate::limine;

struct ImagePixels(UnsafeCell<[u32; MAX_PIXELS]>);
// SAFETY: one bootstrap CPU calls initialize before entering the Desktop
// event loop; image bytes are never written again and readers are passive.
// This is NOT a general SMP-safe, mutating image-storage abstraction.
unsafe impl Sync for ImagePixels {}

static PIXELS: ImagePixels = ImagePixels(UnsafeCell::new([0; MAX_PIXELS]));
static READY: AtomicBool = AtomicBool::new(false);
static WIDTH: AtomicUsize = AtomicUsize::new(0);
static HEIGHT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
pub struct ImageView {
    pub width: usize,
    pub height: usize,
    pub pixels: &'static [u32],
}

/// Return false for absent, malformed, oversized or unsupported boot asset.
/// A missing/corrupt image must never interrupt the normal GOP desktop.
pub fn initialize() -> bool {
    if READY.load(Ordering::Acquire) {
        return true;
    }
    let decoded = limine::with_wallpaper_module(|bytes| {
        let info = ImageInfo::inspect(bytes)?;
        let pixel_count = info.width * info.height;
        // SAFETY: fixed BSS storage is MAX_PIXELS and inspect bounds both
        // dimensions; no other thread reads it until READY is published.
        let dst = unsafe { &mut *PIXELS.0.get() };
        info.decode_into(bytes, &mut dst[..pixel_count])?;
        WIDTH.store(info.width, Ordering::Relaxed);
        HEIGHT.store(info.height, Ordering::Relaxed);
        READY.store(true, Ordering::Release);
        Ok::<(), bmp::BmpError>(())
    });
    matches!(decoded, Some(Ok(())))
}

pub fn current() -> Option<ImageView> {
    if !READY.load(Ordering::Acquire) { return None; }
    let width = WIDTH.load(Ordering::Relaxed);
    let height = HEIGHT.load(Ordering::Relaxed);
    let count = width.checked_mul(height)?;
    if count == 0 || count > MAX_PIXELS { return None; }
    // SAFETY: ready is published only after the full image copy. No writes
    // occur after publication; the memory belongs to kernel BSS, not Limine.
    let pixels = unsafe { &*PIXELS.0.get() };
    Some(ImageView { width, height, pixels: &pixels[..count] })
}
