//! Early ZillaOS GUI renderer over Limine's validated RGB32 framebuffer.
//! This draws directly into boot framebuffer memory (single-core demo only).
//! Production GUI must run in a user-space compositor after process isolation.

use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crate::limine::{self, Framebuffer};
use zillaos_kernel::desktop::{App, Desktop, Layout, Pointer};
use zillaos_kernel::raster::{self, Region};
use zillaos_kernel::window_manager::{Rect, WindowMode};

// Fixed-size offscreen RGB32 surface: 1280 x 800 x 4 = 4,096,000 bytes.
// It resides in the kernel's BSS, NEVER on the stack or a heap.
// Larger Limine resolutions fall back to the original bounded direct renderer.
struct Backbuffer(UnsafeCell<[u32; raster::BACKBUFFER_PIXELS]>);

// SAFETY: the preemptive scheduler/IRQ handler/SMP renderer does not exist.
// Exactly one bootstrap CPU accesses this static through Screen methods.
unsafe impl Sync for Backbuffer {}
static BACKBUFFER: Backbuffer = Backbuffer(UnsafeCell::new([0; raster::BACKBUFFER_PIXELS]));
static BACKBUFFER_VALID: AtomicBool = AtomicBool::new(false);
static BACKBUFFER_WIDTH: AtomicUsize = AtomicUsize::new(0);
static BACKBUFFER_HEIGHT: AtomicUsize = AtomicUsize::new(0);

struct Screen {
    fb: Framebuffer,
    buffered: bool,
}

impl Screen {
    #[inline]
    fn rgb(&self, rgb: u32) -> u32 {
        (((rgb >> 16) & 0xff) << self.fb.red_shift)
            | (((rgb >> 8) & 0xff) << self.fb.green_shift)
            | ((rgb & 0xff) << self.fb.blue_shift)
    }

    #[inline]
    fn rectangle(&self, x: usize, y: usize, width: usize, height: usize, rgb: u32) {
        let Some(region) = Region::new(x, y, width, height)
            .clipped(self.fb.width, self.fb.height) else { return };
        let pixel = self.rgb(rgb);
        if self.buffered {
            // SAFETY: a single early-boot CPU is the sole reader/writer of
            // this static. The resolution fits 1280x800, and Region::clipped
            // guarantees each accessed span is in [0, BACKBUFFER_PIXELS).
            let buffer = unsafe { &mut *BACKBUFFER.0.get() };
            raster::fill_rect(
                buffer,
                raster::BACKBUFFER_WIDTH,
                self.fb.width,
                self.fb.height,
                region,
                pixel,
            );
        } else {
            self.write_direct(region, pixel);
        }
    }

    /// Write pixels to the firmware-mapped display, bypassing the backing
    /// layer. Reserved for flush and drawing the hardware-visible cursor.
    fn write_direct(&self, region: Region, color: u32) {
        let Some(region) = region.clipped(self.fb.width, self.fb.height) else { return };
        for row in region.y..region.y + region.height {
            for col in region.x..region.x + region.width {
                let byte_offset = row * self.fb.pitch + col * 4;
                // SAFETY: framebuffer() validates pixel stride, alignment
                // and mapped bounds. Region stays inside width and height.
                unsafe {
                    ptr::write_volatile(self.fb.address.add(byte_offset).cast::<u32>(), color);
                }
            }
        }
    }

    /// Transfer previously composed background pixels from the BSS buffer
    /// into a clipped framebuffer area. Also restores cursor damage.
    fn flush_region(&self, region: Region) {
        if !self.buffered { return; }
        let Some(region) = region.clipped(self.fb.width, self.fb.height) else { return };
        // SAFETY: render/flush/cursor calls never run concurrently; bounded
        // Region indices are within the statically allocated backing buffer.
        let buffer = unsafe { &*BACKBUFFER.0.get() };
        for row in region.y..region.y + region.height {
            let start = row * raster::BACKBUFFER_WIDTH + region.x;
            for col in 0..region.width {
                let color = buffer[start + col];
                let byte_offset = row * self.fb.pitch + (region.x + col) * 4;
                // SAFETY: firmware RGB32 surface was validated by Limine.
                unsafe {
                    ptr::write_volatile(self.fb.address.add(byte_offset).cast::<u32>(), color);
                }
            }
        }
    }


    fn round_rect(&self, x: usize, y: usize, w: usize, h: usize, radius: usize, rgb: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let r = radius.min(w / 2).min(h / 2);
        for row in 0..h {
            let inset = if row < r {
                (r - row) / 2
            } else if row >= h - r {
                (row - (h - r) + 1) / 2
            } else {
                0
            };
            self.rectangle(x.saturating_add(inset), y.saturating_add(row),
                           w.saturating_sub(2 * inset), 1, rgb);
        }
    }

    fn text(&self, text: &str, x: usize, y: usize, scale: usize, rgb: u32) {
        if scale == 0 {
            return;
        }
        let mut px = x;
        for c in text.bytes() {
            if px.saturating_add(5 * scale) > self.fb.width {
                break;
            }
            let glyph = glyph(c.to_ascii_uppercase());
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..5 {
                    if (*bits & (1u8 << (4 - column))) != 0 {
                        self.rectangle(px + column * scale, y + row * scale,
                                       scale, scale, rgb);
                    }
                }
            }
            px = px.saturating_add(6 * scale);
        }
    }

    fn background(&self, dark: bool) {
        let h = self.fb.height;
        let w = self.fb.width;
        for row in 0..h {
            let v = ((row * 120) / h.max(1)) as u32;
            let rgb = if dark {
                ((6 + v / 12) << 16) | ((16 + v / 5) << 8) | (34 + v / 3)
            } else {
                ((180 + v / 3) << 16) | ((205 + v / 4) << 8) | (236 + v / 8)
            };
            self.rectangle(0, row, w, 1, rgb);
        }
        // Soft beams / horizon, pure flat-color geometry for now.
        self.rectangle(w / 6, h * 62 / 100, w * 2 / 3, 2, 0x187b88);
        self.rectangle(w / 5, h * 64 / 100, w * 3 / 5, 1, 0x145061);
        self.rectangle(w / 3, h * 20 / 100, 3, h / 3, 0x195674);
    }

    fn top_panel(&self, state: &Desktop, layout: &Layout) {
        let fg = if state.dark { 0xebf5ff } else { 0x142739 };
        let background = if state.dark { 0x122138 } else { 0xf5faff };
        self.rectangle(0, 0, layout.width, layout.top_bar, background);
        self.rectangle(0, layout.top_bar - 2, layout.width, 2, 0x228f9c);
        self.round_rect(16, 9, 23, 23, 7, 0x19c7b0);
        self.text("Z", 22, 13, 2, 0x0b2535);
        self.text("ZILLAOS", 49, 15, 2, fg);
        if layout.width >= 720 {
            self.text("DESKTOP   FILE   VIEW   WINDOW", 170, 18, 1, fg);
            self.text("DEV / QEMU", layout.width - 130, 17, 1, fg);
        }
    }

    fn wallpaper_label(&self, state: &Desktop, layout: &Layout) {
        let fg = if state.dark { 0xecfcff } else { 0x17344b };
        let small = if state.dark { 0x80adc4 } else { 0x35566d };
        if layout.width >= 660 && layout.height >= 480 {
            self.text("MAKE SOMETHING GREAT.", 42, layout.top_bar + 46, 2, fg);
            self.text("ZILLA DESKTOP  /  EARLY PREVIEW", 44, layout.top_bar + 75, 1, small);
        }
    }

    fn dock(&self, state: &Desktop, layout: &Layout) {
        let width = layout.width.min(500).saturating_sub(22);
        let x = layout.width.saturating_sub(width) / 2;
        let h = layout.dock_height;
        let y = layout.height.saturating_sub(h + 10);
        let background = if state.dark { 0x192c42 } else { 0xe5f4f8 };
        self.round_rect(x + 3, y + 6, width, h, 17, 0x102036);
        self.round_rect(x, y, width, h, 17, background);
        let icon = h.saturating_sub(18).min(46);
        let spacing = icon + 14;
        let base = x + width.saturating_sub(3 * spacing) / 2 + 7;
        for i in 0..3 {
            let px = base + i * spacing;
            let (app, color, letter) = match i {
                0 => (App::Files, 0x18a9c1, "F"),
                1 => (App::Terminal, 0x425d81, "T"),
                _ => (App::Settings, 0x7d6bd1, "S"),
            };
            self.round_rect(px, y + 8, icon, icon, 11, color);
            self.text(letter, px + (icon.saturating_sub(10)) / 2, y + 17, 2, 0xffffff);
            if state.active() == Some(app) {
                self.rectangle(px + 8, y + h - 5, icon.saturating_sub(16), 3, 0x1cd9bd);
            } else if state.windows.window(app).mode == WindowMode::Minimized {
                self.rectangle(px + icon / 2, y + h - 5, 4, 3, 0xffc35a);
            } else if state.windows.window(app).visible() {
                self.rectangle(px + icon / 2, y + h - 5, 4, 3, 0x7d9cb7);
            }
        }
    }

    fn window(&self, state: &Desktop, app: App, rect: Rect, focused: bool) {
        let x = rect.x;
        let y = rect.y;
        let w = rect.width;
        let h = rect.height;
        let bg = if state.dark { 0x17253a } else { 0xf5faff };
        let title_bar = match (state.dark, focused) {
            (true, true) => 0x2f5270,
            (true, false) => 0x253b54,
            (false, true) => 0xd0e7f2,
            (false, false) => 0xdce9f2,
        };
        let fg = if state.dark { 0xedf5fc } else { 0x20394e };
        let secondary = if state.dark { 0x9eb5cc } else { 0x587084 };
        self.round_rect(x + 8, y + 11, w, h, 13, 0x0b192b);
        if focused {
            self.round_rect(x.saturating_sub(2), y.saturating_sub(2),
                            w.saturating_add(4), h.saturating_add(4), 13, 0x18788d);
        }
        self.round_rect(x, y, w, h, 13, bg);
        let bar_h = if h < 170 { 30 } else { 45 };
        self.round_rect(x, y, w, bar_h, 13, title_bar);
        self.rectangle(x, y + bar_h - 12, w, 12, title_bar);
        for (i, dot) in [0xfd6872, 0xffcb55, 0x45c784].iter().enumerate() {
            self.round_rect(x + 14 + i * 19, y + 13, 11, 11, 5, *dot);
        }
        self.text(app.label(), x + 87, y + 14, 2, fg);
        if focused && w >= 310 {
            self.text("ACTIVE", x + w.saturating_sub(70), y + 18, 1, 0x1bd5ba);
        }

        let body = y + bar_h + 8;
        let content_h = h.saturating_sub(bar_h + 16);
        if content_h < 36 {
            return;
        }
        let side_width = (w / 4).clamp(70, 170);
        self.round_rect(x + 10, body, side_width, content_h, 9,
                        if state.dark { 0x20344c } else { 0xe7f2f8 });
        let ty = body + 15;
        self.text("HOME", x + 19, ty, 1, secondary);
        if content_h > 105 {
            self.text("FILES", x + 19, ty + 24, 1, fg);
            self.text("APPS", x + 19, ty + 46, 1, fg);
            self.text("ABOUT", x + 19, ty + 68, 1, fg);
        }

        let cx = x + side_width + 27;
        let cw = w.saturating_sub(side_width + 44);
        match app {
            App::Files => {
                self.text("ZILLA FILES", cx, body + 10, 2, fg);
                if content_h > 100 {
                    let card_y = body + 42;
                    self.folder_card(cx, card_y, cw, "DOCUMENTS", state.dark);
                    if content_h > 175 {
                        self.folder_card(cx, card_y + 47, cw, "PICTURES", state.dark);
                        self.folder_card(cx, card_y + 94, cw, "PROJECTS", state.dark);
                    }
                }
            }
            App::Terminal => {
                self.round_rect(cx, body + 1, cw, content_h.saturating_sub(3), 8, 0x0b1726);
                self.text("ZILLA TERMINAL", cx + 12, body + 12, 1, 0x1dd9b4);
                if content_h > 105 {
                    self.text("> ZILLAOS 0.0.2", cx + 12, body + 39, 1, 0xc4dbea);
                    self.text("> RUST / X86_64", cx + 12, body + 58, 1, 0xc4dbea);
                    self.text("> GUI PREVIEW", cx + 12, body + 77, 1, 0x1dd9b4);
                }
            }
            App::Settings => {
                self.text("SETTINGS", cx, body + 10, 2, fg);
                if content_h > 98 {
                    self.text("APPEARANCE", cx, body + 44, 1, secondary);
                    let theme = if state.dark { "DARK THEME" } else { "LIGHT THEME" };
                    self.text(theme, cx, body + 65, 1, fg);
                    if content_h > 160 {
                        self.text("F5 TO SWITCH", cx, body + 105, 1, 0x1ed2b4);
                        self.text("DISPLAY: RGB32", cx, body + 126, 1, secondary);
                    }
                }
            }
        }
    }

    fn folder_card(&self, x: usize, y: usize, max_width: usize, title: &str, dark: bool) {
        if max_width < 70 {
            return;
        }
        self.round_rect(x, y, max_width.min(360), 40, 7,
                        if dark { 0x283e55 } else { 0xe1edf5 });
        self.round_rect(x + 10, y + 12, 19, 15, 3, 0x2bc5be);
        self.rectangle(x + 13, y + 9, 11, 5, 0x58dce0);
        self.text(title, x + 38, y + 16, 1, if dark { 0xebf5ff } else { 0x173047 });
    }

    fn launcher(&self, state: &Desktop, layout: &Layout) {
        if !state.launcher { return; }
        let w = layout.width.saturating_sub(40).min(410);
        let x = layout.width.saturating_sub(w) / 2;
        let y = layout.top_bar + 16;
        let h = 158.min(layout.height.saturating_sub(y + 10));
        self.round_rect(x + 5, y + 7, w, h, 12, 0x071526);
        self.round_rect(x, y, w, h, 12,
                        if state.dark { 0x233850 } else { 0xf0faff });
        let fg = if state.dark { 0xffffff } else { 0x183144 };
        self.text("ZILLA LAUNCHER", x + 18, y + 17, 2, fg);
        if h >= 120 {
            self.text("F2  FILES", x + 18, y + 55, 2, fg);
            self.text("F3  TERMINAL", x + 18, y + 79, 2, fg);
            self.text("F4  SETTINGS", x + 18, y + 103, 2, fg);
        }
    }

    /// A small software cursor. Always rendered LAST so no window or
    /// launcher can cover it. Rectangle clipping keeps every pixel safe.
    fn cursor(&self, state: &Desktop) {
        if !state.pointer.visible { return; }
        let x = state.pointer.x;
        let y = state.pointer.y;
        // Draw directly, AFTER committing the background buffer. The cursor
        // must never be saved into the backing pixels that will restore it.
        for row in 0..17usize {
            let width = if row < 12 { 2 + row / 2 } else { 3 };
            self.write_direct(Region::new(x, y.saturating_add(row), width + 2, 1),
                              self.rgb(0x092134));
            self.write_direct(Region::new(x.saturating_add(1), y.saturating_add(row),
                                          width, 1), self.rgb(0xf4fbff));
        }
        self.write_direct(Region::new(x.saturating_add(2), y.saturating_add(13),
                                      3, 5), self.rgb(0x1dd9b4));
    }

    fn key_legend(&self, state: &Desktop, layout: &Layout) {
        if layout.width < 730 || layout.height < 460 { return; }
        let fg = if state.dark { 0xa5c7d8 } else { 0x244d64 };
        self.text("F1 MENU F2 FILES F3 TERM F4 SETTINGS F5 THEME F6 MIN F7 MAX TAB FOCUS ESC CLOSE",
                  22, layout.height.saturating_sub(12), 1, fg);
        if layout.height >= 600 && layout.width >= 820 {
            self.text("ARROWS MOVE  SHIFT ARROWS RESIZE",
                      22, layout.height.saturating_sub(layout.dock_height + 25), 1, fg);
        }
    }
}

/// Draw one entire desktop frame. The caller drives redraw on input events.
/// Returns false when the bootloader framebuffer cannot be used.
pub fn render(state: &Desktop) -> bool {
    let Some(fb) = limine::framebuffer() else { return false };
    let Some(layout) = Layout::for_size(fb.width, fb.height) else { return false };
    let buffered = raster::supports_backbuffer(fb.width, fb.height);
    BACKBUFFER_VALID.store(false, Ordering::Release);
    let screen = Screen { fb, buffered };
    screen.background(state.dark);
    screen.wallpaper_label(state, &layout);
    screen.top_panel(state, &layout);
    for app in state.windows.back_to_front() {
        if state.windows.window(app).visible() {
            screen.window(state, app, state.windows.rect(app), state.active() == Some(app));
        }
    }
    screen.dock(state, &layout);
    screen.launcher(state, &layout);
    screen.key_legend(state, &layout);

    if buffered {
        // Compose all GUI layers before a single complete screen transfer.
        screen.flush_region(Region::new(0, 0, screen.fb.width, screen.fb.height));
        BACKBUFFER_WIDTH.store(screen.fb.width, Ordering::Relaxed);
        BACKBUFFER_HEIGHT.store(screen.fb.height, Ordering::Relaxed);
        BACKBUFFER_VALID.store(true, Ordering::Release);
    }
    screen.cursor(state);
    true
}

/// Whether the most recent complete scene is cached in BSS.
pub fn has_backbuffer() -> bool {
    BACKBUFFER_VALID.load(Ordering::Acquire)
}

/// Fast path for pointer movement without changing windows/theme/launcher.
/// Restore the old and new cursor rectangles only (<=480 pixels total).
/// Returns false when no cached background is available, in which case the
/// caller MUST redraw the complete desktop.
pub fn redraw_cursor(previous: Pointer, state: &Desktop) -> bool {
    if !state.pointer.visible || !previous.visible || !has_backbuffer() {
        return false;
    }
    let Some(fb) = limine::framebuffer() else { return false };
    if !raster::supports_backbuffer(fb.width, fb.height)
        || BACKBUFFER_WIDTH.load(Ordering::Relaxed) != fb.width
        || BACKBUFFER_HEIGHT.load(Ordering::Relaxed) != fb.height
    {
        return false;
    }

    let screen = Screen { fb, buffered: true };
    if let Some(region) = Region::cursor_at(previous.x, previous.y,
                                            screen.fb.width, screen.fb.height) {
        screen.flush_region(region);
    }
    if let Some(region) = Region::cursor_at(state.pointer.x, state.pointer.y,
                                            screen.fb.width, screen.fb.height) {
        screen.flush_region(region);
    }
    screen.cursor(state);
    true
}

fn glyph(c: u8) -> [u8; 7] {
    match c {
        b'A' => [14,17,17,31,17,17,17],
        b'B' => [30,17,17,30,17,17,30],
        b'C' => [15,16,16,16,16,16,15],
        b'D' => [30,17,17,17,17,17,30],
        b'E' => [31,16,16,30,16,16,31],
        b'F' => [31,16,16,30,16,16,16],
        b'G' => [15,16,16,23,17,17,15],
        b'H' => [17,17,17,31,17,17,17],
        b'I' => [31,4,4,4,4,4,31],
        b'J' => [7,2,2,2,18,18,12],
        b'K' => [17,18,20,24,20,18,17],
        b'L' => [16,16,16,16,16,16,31],
        b'M' => [17,27,21,21,17,17,17],
        b'N' => [17,25,21,19,17,17,17],
        b'O' => [14,17,17,17,17,17,14],
        b'P' => [30,17,17,30,16,16,16],
        b'Q' => [14,17,17,17,21,18,13],
        b'R' => [30,17,17,30,20,18,17],
        b'S' => [15,16,16,14,1,1,30],
        b'T' => [31,4,4,4,4,4,4],
        b'U' => [17,17,17,17,17,17,14],
        b'V' => [17,17,17,17,17,10,4],
        b'W' => [17,17,17,21,21,21,10],
        b'X' => [17,17,10,4,10,17,17],
        b'Y' => [17,17,10,4,4,4,4],
        b'Z' => [31,1,2,4,8,16,31],
        b'0' => [14,17,19,21,25,17,14],
        b'1' => [4,12,4,4,4,4,14],
        b'2' => [14,17,1,2,4,8,31],
        b'3' => [30,1,1,14,1,1,30],
        b'4' => [2,6,10,18,31,2,2],
        b'5' => [31,16,16,30,1,1,30],
        b'6' => [14,16,16,30,17,17,14],
        b'7' => [31,1,2,4,8,8,8],
        b'8' => [14,17,17,14,17,17,14],
        b'9' => [14,17,17,15,1,1,14],
        b'.' => [0,0,0,0,0,12,12],
        b':' => [0,12,12,0,12,12,0],
        b'/' => [1,2,2,4,8,8,16],
        b'-' => [0,0,0,31,0,0,0],
        b'>' => [16,8,4,2,4,8,16],
        b'_' => [0,0,0,0,0,0,31],
        b' ' => [0;7],
        _ => [0;7],
    }
}
