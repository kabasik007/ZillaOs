//! Windows-like sliding Start panel geometry.
//! This is a pure no_std animation model, independent of framebuffer I/O.
use crate::desktop::Layout;

pub const STEPS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

pub fn panel(layout: Layout, progress: u8) -> Panel {
    let width = layout.width.saturating_sub(32).min(360);
    let available = layout.height.saturating_sub(layout.top_bar + layout.dock_height + 26);
    let height = available.min(158);
    let base = layout.height.saturating_sub(layout.dock_height + height + 22)
        .max(layout.top_bar + 8);
    let hidden = height.saturating_add(24);
    let step = progress.min(STEPS) as usize;
    let offset = hidden.saturating_mul(STEPS as usize - step) / STEPS as usize;
    Panel { x: 16, y: base.saturating_add(offset), width, height }
}

pub fn app_row(layout: Layout, progress: u8, x: usize, y: usize) -> Option<usize> {
    if progress == 0 { return None; }
    let rect = panel(layout, progress);
    if x < rect.x || x >= rect.x.saturating_add(rect.width)
        || y < rect.y || y >= rect.y.saturating_add(rect.height) {
        return None;
    }
    match y - rect.y {
        50..=77 => Some(0),
        78..=101 => Some(1),
        102..=131 => Some(2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn start_panel_slides_from_taskbar_without_moving_sideways() {
        let layout = Layout::for_size(1024, 768).unwrap();
        let hidden = panel(layout, 0);
        let shown = panel(layout, 8);
        assert_eq!(hidden.x, shown.x);
        assert!(hidden.y > shown.y);
        assert!(shown.y >= layout.top_bar);
        assert!(shown.y + shown.height <= layout.height - layout.dock_height);
        let mut last = hidden.y;
        for step in 1..=STEPS {
            let current = panel(layout, step);
            assert!(current.y <= last);
            last = current.y;
        }
        assert_eq!(app_row(layout, 8, shown.x + 40, shown.y + 112), Some(2));
        assert_eq!(app_row(layout, 0, shown.x + 40, shown.y + 112), None);
    }
    #[test]
    fn panel_geometry_is_bounded_on_small_and_hd_displays() {
        for (w,h) in [(320,240),(640,480),(800,600),(1024,768),(1920,1080)] {
            let layout = Layout::for_size(w,h).unwrap();
            let p = panel(layout, STEPS);
            assert!(p.x + p.width <= w);
            assert!(p.y + p.height <= h);
        }
    }
}
