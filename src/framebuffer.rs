//! A tiny bounded framebuffer splash for QEMU/OVMF.
//! This is NOT a compositor, GPU driver or an interactive desktop.
//! The bootloader maps the video buffer. Single CPU, during early boot.

use crate::limine::Framebuffer;
use core::ptr;

struct Canvas(Framebuffer);

impl Canvas {
    fn rgb(&self, red: u8, green: u8, blue: u8) -> u32 {
        ((red as u32) << self.0.red_shift)
            | ((green as u32) << self.0.green_shift)
            | ((blue as u32) << self.0.blue_shift)
    }

    fn rect(&self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let end_y = y.saturating_add(h).min(self.0.height);
        let end_x = x.saturating_add(w).min(self.0.width);
        for row in y.min(self.0.height)..end_y {
            for col in x.min(self.0.width)..end_x {
                let byte_offset = row * self.0.pitch + col * 4;
                // SAFETY: limine::framebuffer() validates address alignment,
                // the RGB32 layout, dimensions, pitch and total mapped size.
                // Writes are within the bootloader-provided framebuffer.
                unsafe {
                    ptr::write_volatile(self.0.address.add(byte_offset).cast::<u32>(), color);
                }
            }
        }
    }

    fn draw_letter(&self, ch: u8, x: usize, y: usize, scale: usize, color: u32) {
        let glyph = match ch {
            b'Z' => [31, 1, 2, 4, 8, 16, 31],
            b'I' => [31, 4, 4, 4, 4, 4, 31],
            b'L' => [16, 16, 16, 16, 16, 16, 31],
            b'A' => [14, 17, 17, 31, 17, 17, 17],
            b'O' => [14, 17, 17, 17, 17, 17, 14],
            b'S' => [15, 16, 16, 14, 1, 1, 30],
            _ => [0; 7],
        };
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (*bits & (1_u8 << (4 - col))) != 0 {
                    self.rect(x + col * scale, y + row * scale, scale, scale, color);
                }
            }
        }
    }
}

/// Render a splash only if Limine supplied a framebuffer we can safely
/// bound and interpret as RGB32. Serial boot remains functional without it.
pub fn render() -> bool {
    let Some(framebuffer) = crate::limine::framebuffer() else {
        return false;
    };
    let canvas = Canvas(framebuffer);
    let w = canvas.0.width;
    let h = canvas.0.height;
    let dark = canvas.rgb(10, 18, 33);
    let dark_lower = canvas.rgb(15, 28, 48);
    let cyan = canvas.rgb(45, 211, 184);
    let white = canvas.rgb(235, 243, 250);
    let muted = canvas.rgb(82, 110, 142);

    canvas.rect(0, 0, w, h, dark);
    canvas.rect(0, h / 2, w, h - h / 2, dark_lower);
    canvas.rect(0, 0, w, 4, cyan);
    let scale = (w / 120).clamp(2, 9);
    let caption_width = (7 * 6 - 1) * scale;
    let left = w.saturating_sub(caption_width) / 2;
    let top = h.saturating_sub(7 * scale) / 2;
    for (i, ch) in b"ZILLAOS".iter().enumerate() {
        canvas.draw_letter(*ch, left + i * 6 * scale, top, scale, white);
    }
    let bar_width = (w / 3).min(400);
    let bar_x = w.saturating_sub(bar_width) / 2;
    let bar_y = (top + 7 * scale + 28).min(h.saturating_sub(8));
    canvas.rect(bar_x, bar_y, bar_width, 3, muted);
    canvas.rect(bar_x, bar_y, bar_width / 3, 3, cyan);
    true
}
