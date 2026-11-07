//! Movable, clickable folder shortcuts on the experimental ZillaOS desktop.
//!
//! The icons point to immutable guest RAMFS directory IDs. They never expose
//! Windows host files, real disk contents or the kernel's source repository.
use crate::desktop::Layout;
use crate::fs::{self, NodeKind};

pub const ICON_WIDTH: usize = 92;
pub const ICON_HEIGHT: usize = 70;
pub const ICON_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderShortcut {
    pub title: &'static str,
    pub path: &'static str,
}

pub const FOLDERS: [FolderShortcut; ICON_COUNT] = [
    FolderShortcut {
        title: "DOCUMENTS",
        path: "/Users/Guest/Documents",
    },
    FolderShortcut {
        title: "PICTURES",
        path: "/Users/Guest/Pictures",
    },
    FolderShortcut {
        title: "DOWNLOADS",
        path: "/Users/Guest/Downloads",
    },
    FolderShortcut {
        title: "PROGRAM FILES",
        path: "/Program Files",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconPoint {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopIcons {
    pub positions: [IconPoint; ICON_COUNT],
    pub dragging: Option<usize>,
    pub moved: bool,
    width: usize,
    height: usize,
    min_y: usize,
    bottom: usize,
}

impl DesktopIcons {
    pub fn new(layout: Layout) -> Self {
        let x = layout.width.saturating_sub(ICON_WIDTH + 22);
        let min_y = layout.top_bar.saturating_add(18);
        let bottom = layout.height.saturating_sub(layout.dock_height + 18);
        let mut positions = [IconPoint { x, y: min_y }; ICON_COUNT];
        for (index, p) in positions.iter_mut().enumerate() {
            p.y = min_y
                .saturating_add(22 + index * 82)
                .min(bottom.saturating_sub(ICON_HEIGHT));
        }
        Self {
            positions,
            dragging: None,
            moved: false,
            width: layout.width,
            height: layout.height,
            min_y,
            bottom,
        }
    }

    pub fn visible(&self) -> bool {
        self.width >= 540 && self.height >= 500
    }

    pub fn hit(&self, x: usize, y: usize) -> Option<usize> {
        if !self.visible() {
            return None;
        }
        self.positions
            .iter()
            .enumerate()
            .rev()
            .find_map(|(id, point)| {
                if x >= point.x
                    && x < point.x.saturating_add(ICON_WIDTH)
                    && y >= point.y
                    && y < point.y.saturating_add(ICON_HEIGHT)
                {
                    Some(id)
                } else {
                    None
                }
            })
    }

    pub fn begin(&mut self, icon: usize) -> bool {
        if icon >= ICON_COUNT || !self.visible() {
            return false;
        }
        self.dragging = Some(icon);
        self.moved = false;
        true
    }

    pub fn move_by(&mut self, dx: i16, dy: i16) -> bool {
        let Some(icon) = self.dragging else {
            return false;
        };
        if dx == 0 && dy == 0 {
            return false;
        }
        let point = &mut self.positions[icon];
        let x = point
            .x
            .saturating_add_signed(dx as isize)
            .min(self.width.saturating_sub(ICON_WIDTH));
        let y = point.y.saturating_add_signed(dy as isize).clamp(
            self.min_y,
            self.bottom.saturating_sub(ICON_HEIGHT).max(self.min_y),
        );
        if (x, y) != (point.x, point.y) {
            point.x = x;
            point.y = y;
            self.moved = true;
            return true;
        }
        false
    }

    /// Return the guest directory ID only for a click without dragging.
    pub fn finish(&mut self) -> Option<usize> {
        let icon = self.dragging.take()?;
        let moved = self.moved;
        self.moved = false;
        if moved {
            return None;
        }
        let id = fs::lookup(FOLDERS[icon].path)?;
        if fs::node(id)?.kind != NodeKind::Directory {
            return None;
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icons_reference_existing_guest_directories() {
        let icons = DesktopIcons::new(Layout::for_size(1024, 768).unwrap());
        for (index, folder) in FOLDERS.iter().enumerate() {
            assert_eq!(
                icons.hit(icons.positions[index].x + 5, icons.positions[index].y + 5),
                Some(index)
            );
            let id = fs::lookup(folder.path).unwrap();
            assert_eq!(fs::node(id).unwrap().kind, NodeKind::Directory);
        }
    }
    #[test]
    fn click_opens_only_directory_and_drag_only_moves_on_screen() {
        let mut icons = DesktopIcons::new(Layout::for_size(1024, 768).unwrap());
        assert!(icons.begin(0));
        let documents = icons.finish().unwrap();
        assert_eq!(fs::node(documents).unwrap().path, "/Users/Guest/Documents");
        assert!(icons.begin(0));
        assert!(icons.move_by(-300, 34));
        assert_eq!(icons.finish(), None);
        assert!(icons.positions[0].x < 900);
        assert!(!icons.begin(100));
        assert!(!icons.move_by(5, 5));
        assert_eq!(icons.finish(), None);
        assert!(icons.begin(3));
        assert!(icons.move_by(i16::MAX, i16::MAX));
        assert!(icons.positions[3].x + ICON_WIDTH <= 1024);
        assert!(icons.positions[3].y + ICON_HEIGHT <= 768);
    }
    #[test]
    fn shortcuts_are_disabled_in_small_ui_and_without_disk_io() {
        let icons = DesktopIcons::new(Layout::for_size(320, 240).unwrap());
        assert!(!icons.visible());
        assert_eq!(icons.hit(5, 5), None);
    }
}
