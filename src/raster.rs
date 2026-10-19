//! Testable graphics geometry for a bounded, heap-free software compositor.
//! The boot demo uses this for clipping, RGB32 backbuffer decisions and
//! restoring cursor-damaged rectangles without repainting the whole desktop.

pub const BACKBUFFER_WIDTH: usize = 1280;
pub const BACKBUFFER_HEIGHT: usize = 800;
pub const BACKBUFFER_PIXELS: usize = BACKBUFFER_WIDTH * BACKBUFFER_HEIGHT;
pub const CURSOR_WIDTH: usize = 12;
pub const CURSOR_HEIGHT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Region {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self { x, y, width, height }
    }

    pub fn clipped(self, width: usize, height: usize) -> Option<Self> {
        let x0 = self.x.min(width);
        let y0 = self.y.min(height);
        let x1 = self.x.saturating_add(self.width).min(width);
        let y1 = self.y.saturating_add(self.height).min(height);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some(Self::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// Returns exactly the area that a software cursor can overwrite.
    /// No unchecked coordinate addition, even for hostile large inputs.
    pub fn cursor_at(x: usize, y: usize, width: usize, height: usize) -> Option<Self> {
        Self::new(x, y, CURSOR_WIDTH, CURSOR_HEIGHT).clipped(width, height)
    }
}

pub const fn supports_backbuffer(width: usize, height: usize) -> bool {
    width > 0
        && height > 0
        && width <= BACKBUFFER_WIDTH
        && height <= BACKBUFFER_HEIGHT
}

pub fn backing_index(row: usize, col: usize) -> Option<usize> {
    if row >= BACKBUFFER_HEIGHT || col >= BACKBUFFER_WIDTH {
        return None;
    }
    row.checked_mul(BACKBUFFER_WIDTH)?.checked_add(col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_at_all_screen_edges() {
        assert_eq!(
            Region::new(300, 190, 50, 50).clipped(320, 200),
            Some(Region::new(300, 190, 20, 10)),
        );
        assert_eq!(Region::new(320, 1, 5, 5).clipped(320, 200), None);
        assert_eq!(Region::new(1, 200, 5, 5).clipped(320, 200), None);
        assert_eq!(Region::new(0, 0, 0, 10).clipped(320, 200), None);
    }

    #[test]
    fn gigantic_coordinates_are_clamped_without_overflow() {
        assert_eq!(Region::new(319, 199, usize::MAX, usize::MAX).clipped(320, 200),
            Some(Region::new(319, 199, 1, 1)));
        assert_eq!(Region::new(usize::MAX, 0, 20, 20).clipped(320, 200), None);
    }

    #[test]
    fn cursor_damage_never_leaves_framebuffer() {
        assert_eq!(Region::cursor_at(315, 197, 320, 200),
            Some(Region::new(315, 197, 5, 3)));
        assert_eq!(Region::cursor_at(320, 0, 320, 200), None);
    }

    #[test]
    fn backbuffer_has_explicit_capacity() {
        assert!(supports_backbuffer(1024, 768));
        assert!(supports_backbuffer(1280, 800));
        assert!(!supports_backbuffer(1281, 800));
        assert!(!supports_backbuffer(1280, 801));
        assert_eq!(backing_index(799, 1279), Some(BACKBUFFER_PIXELS - 1));
        assert_eq!(backing_index(800, 0), None);
        assert_eq!(backing_index(0, 1280), None);
    }
}
