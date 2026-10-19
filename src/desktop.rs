//! Zilla Desktop event state and small translated PS/2 keyboard decoder.
//! Real processes and IPC-backed application surfaces are a later milestone.

use crate::window_manager::{Move, WindowManager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Files,
    Terminal,
    Settings,
}

impl App {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Files => "FILES",
            Self::Terminal => "TERMINAL",
            Self::Settings => "SETTINGS",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Launcher,
    Files,
    Terminal,
    Settings,
    ToggleTheme,
    NextApp,
    Minimize,
    Maximize,
    Move(Move),
    Resize(Move),
    Escape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    pub top_bar: usize,
    pub dock_height: usize,
    pub window_x: usize,
    pub window_y: usize,
    pub window_width: usize,
    pub window_height: usize,
}

impl Layout {
    pub fn for_size(width: usize, height: usize) -> Option<Self> {
        if width < 320 || height < 200 {
            return None;
        }
        let top_bar = if height < 400 { 30 } else { 42 };
        let dock_height = if height < 400 { 46 } else { 72 };
        let gap = if height < 400 { 8 } else { 26 };
        let max_width = width.saturating_sub(32);
        let window_width = max_width.min(780);
        let max_height = height.saturating_sub(top_bar + dock_height + gap * 2);
        let window_height = max_height.min(480);
        if window_width < 240 || window_height < 50 {
            return None;
        }
        let window_x = (width - window_width) / 2;
        let window_y = top_bar + gap;
        Some(Self {
            width,
            height,
            top_bar,
            dock_height,
            window_x,
            window_y,
            window_width,
            window_height,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desktop {
    pub windows: WindowManager,
    pub launcher: bool,
    pub dark: bool,
    pub changes: u32,
}

impl Default for Desktop {
    fn default() -> Self {
        Self::new()
    }
}

impl Desktop {
    /// Default viewport used by host tests and when no framebuffer exists.
    pub fn new() -> Self {
        Self::with_layout(Layout::for_size(1024, 768).expect("valid default desktop size"))
    }

    pub fn with_layout(layout: Layout) -> Self {
        Self {
            windows: WindowManager::new(layout),
            launcher: false,
            dark: true,
            changes: 0,
        }
    }

    pub const fn active(&self) -> Option<App> {
        self.windows.focused()
    }

    /// Return true only when a state transition changes the visible desktop.
    pub fn handle(&mut self, key: Key) -> bool {
        let before = (self.windows, self.launcher, self.dark);
        match key {
            Key::Launcher => self.launcher = !self.launcher,
            Key::Files => {
                self.windows.open(App::Files);
                self.launcher = false;
            }
            Key::Terminal => {
                self.windows.open(App::Terminal);
                self.launcher = false;
            }
            Key::Settings => {
                self.windows.open(App::Settings);
                self.launcher = false;
            }
            Key::ToggleTheme => self.dark = !self.dark,
            Key::NextApp => {
                self.launcher = false;
                self.windows.cycle();
            }
            Key::Minimize => self.windows.minimize_focused(),
            Key::Maximize => self.windows.toggle_maximize(),
            Key::Move(direction) => self.windows.translate_focused(direction),
            Key::Resize(direction) => self.windows.resize_focused(direction),
            Key::Escape => {
                if self.launcher {
                    self.launcher = false;
                } else {
                    self.windows.close_focused();
                }
            }
        }
        let changed = before != (self.windows, self.launcher, self.dark);
        if changed {
            self.changes = self.changes.wrapping_add(1);
        }
        changed
    }
}

/// Translated scan-set-1 reader. Port I/O belongs to src/input.rs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScancodeDecoder {
    extended: bool,
    shift: bool,
}

impl Default for ScancodeDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ScancodeDecoder {
    pub const fn new() -> Self {
        Self {
            extended: false,
            shift: false,
        }
    }

    pub fn feed(&mut self, code: u8) -> Option<Key> {
        if code == 0xe0 {
            self.extended = true;
            return None;
        }
        if !self.extended && (code == 0x2a || code == 0x36) {
            self.shift = true;
            return None;
        }
        if !self.extended && (code == 0xaa || code == 0xb6) {
            self.shift = false;
            return None;
        }
        let extended = self.extended;
        self.extended = false;

        // Never act on key-up. 0xe1 Pause/Break isn't supported.
        if code & 0x80 != 0 {
            return None;
        }

        if extended {
            let direction = match code {
                0x4b => Move::Left,
                0x4d => Move::Right,
                0x48 => Move::Up,
                0x50 => Move::Down,
                _ => return None,
            };
            return Some(if self.shift {
                Key::Resize(direction)
            } else {
                Key::Move(direction)
            });
        }

        match code {
            0x3b => Some(Key::Launcher),     // F1
            0x3c => Some(Key::Files),        // F2
            0x3d => Some(Key::Terminal),     // F3
            0x3e => Some(Key::Settings),     // F4
            0x3f => Some(Key::ToggleTheme),  // F5
            0x40 => Some(Key::Minimize),     // F6
            0x41 => Some(Key::Maximize),     // F7
            0x0f => Some(Key::NextApp),      // Tab
            0x01 => Some(Key::Escape),       // Escape
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.extended = false;
        self.shift = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_manager::WindowMode;

    #[test]
    fn translated_scan_codes_and_releases_are_stable() {
        let mut decoder = ScancodeDecoder::new();
        assert_eq!(decoder.feed(0x3b), Some(Key::Launcher));
        assert_eq!(decoder.feed(0xbb), None);
        assert_eq!(decoder.feed(0x3c), Some(Key::Files));
        assert_eq!(decoder.feed(0x3d), Some(Key::Terminal));
        assert_eq!(decoder.feed(0x3e), Some(Key::Settings));
        assert_eq!(decoder.feed(0x3f), Some(Key::ToggleTheme));
        assert_eq!(decoder.feed(0x40), Some(Key::Minimize));
        assert_eq!(decoder.feed(0x41), Some(Key::Maximize));
        assert_eq!(decoder.feed(0x0f), Some(Key::NextApp));
        assert_eq!(decoder.feed(0x01), Some(Key::Escape));
        assert_eq!(decoder.feed(0xe0), None);
        assert_eq!(decoder.feed(0x4b), Some(Key::Move(Move::Left)));
        assert_eq!(decoder.feed(0x2a), None);
        assert_eq!(decoder.feed(0xe0), None);
        assert_eq!(decoder.feed(0x4d), Some(Key::Resize(Move::Right)));
        assert_eq!(decoder.feed(0xaa), None);
        assert_eq!(decoder.feed(0xe0), None);
        assert_eq!(decoder.feed(0x4d), Some(Key::Move(Move::Right)));
        assert_eq!(decoder.feed(0xcd), None);
    }

    #[test]
    fn opens_multiple_windows_and_cycles_only_visible() {
        let mut state = Desktop::new();
        assert_eq!(state.active(), Some(App::Files));
        assert!(state.handle(Key::Terminal));
        assert!(state.handle(Key::Settings));
        assert_eq!(state.windows.visible_count(), 3);
        assert_eq!(state.active(), Some(App::Settings));
        assert!(state.handle(Key::NextApp));
        assert_eq!(state.active(), Some(App::Files));
        assert!(state.handle(Key::NextApp));
        assert_eq!(state.active(), Some(App::Terminal));
    }

    #[test]
    fn launcher_escape_and_minimize_behave_independently() {
        let mut state = Desktop::new();
        assert!(state.handle(Key::Launcher));
        assert!(state.handle(Key::Escape));
        assert_eq!(state.active(), Some(App::Files));
        assert!(state.handle(Key::Minimize));
        assert_eq!(state.active(), None);
        assert_eq!(state.windows.window(App::Files).mode, WindowMode::Minimized);
        assert!(state.handle(Key::Files));
        assert_eq!(state.active(), Some(App::Files));
        assert!(state.handle(Key::Escape));
        assert_eq!(state.active(), None);
    }

    #[test]
    fn theme_toggle_preserves_windows() {
        let mut state = Desktop::new();
        state.handle(Key::Terminal);
        assert!(state.handle(Key::ToggleTheme));
        assert!(!state.dark);
        assert_eq!(state.windows.visible_count(), 2);
        assert!(state.handle(Key::ToggleTheme));
        assert!(state.dark);
    }

    #[test]
    fn move_resize_and_maximize_have_real_effects() {
        let mut state = Desktop::new();
        let old = state.windows.rect(App::Files);
        assert!(state.handle(Key::Move(Move::Right)));
        assert!(state.windows.rect(App::Files).x > old.x);
        assert!(state.handle(Key::Resize(Move::Left)));
        assert!(state.windows.rect(App::Files).width < old.width);
        assert!(state.handle(Key::Maximize));
        assert_eq!(state.windows.window(App::Files).mode, WindowMode::Maximized);
        assert!(!state.handle(Key::Move(Move::Right)));
    }

    #[test]
    fn layout_remains_inside_framebuffer() {
        for (w, h) in [(320, 240), (640, 480), (800, 600), (1024, 768), (1920, 1080)] {
            let layout = Layout::for_size(w, h).unwrap();
            assert!(layout.window_x + layout.window_width <= w);
            assert!(layout.window_y + layout.window_height + layout.dock_height <= h);
        }
        assert!(Layout::for_size(240, 150).is_none());
    }
}
