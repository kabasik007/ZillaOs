//! Zilla Desktop event state and small translated PS/2 keyboard decoder.
//! Real processes and IPC-backed application surfaces are a later milestone.

use crate::mouse::MousePacket;
use crate::fs::Browser;
use crate::locale::{text as tr, TextId};
use crate::setup::{DeviceName, Language, Region, TimeZone};
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
    CycleLanguage,
    ShowDesktop,
    NextApp,
    Minimize,
    Maximize,
    Move(Move),
    Resize(Move),
    Escape,
    Confirm,
    Backspace,
    Text(u8),
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
pub struct Pointer {
    pub x: usize,
    pub y: usize,
    pub visible: bool,
    pub left: bool,
    dragging: Option<App>,
}

impl Pointer {
    pub const fn dragging(&self) -> Option<App> {
        self.dragging
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desktop {
    pub windows: WindowManager,
    pub hostname: DeviceName,
    pub language: Language,
    pub region: Region,
    pub timezone: TimeZone,
    pub pointer: Pointer,
    pub files: Browser,
    layout: Layout,
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
            files: Browser::new(),
            hostname: DeviceName::new(),
            language: Language::Ukrainian,
            region: Region::Ukraine,
            timezone: TimeZone::Kyiv,
            pointer: Pointer {
                x: layout.width / 2,
                y: layout.height / 2,
                visible: false,
                left: false,
                dragging: None,
            },
            layout,
            launcher: false,
            dark: true,
            changes: 0,
        }
    }

    pub const fn active(&self) -> Option<App> {
        self.windows.focused()
    }

    pub fn cycle_language(&mut self) {
        self.language = match self.language {
            Language::Ukrainian => Language::English,
            Language::English => Language::Russian,
            Language::Russian => Language::Ukrainian,
        };
    }

    /// Settings rows share these coordinates with the immediate renderer.
    /// 0 is appearance and 1 is language. Resolution is read-only because
    /// the Limine-provided GOP framebuffer cannot change modes after handoff.
    fn settings_row(rect: crate::window_manager::Rect, x: usize, y: usize) -> Option<usize> {
        let bar_h = if rect.height < 170 { 30 } else { 45 };
        let body = rect.y + bar_h + 8;
        let side = (rect.width / 4).clamp(70, 170);
        let cx = rect.x + side + 27;
        if x < cx || x >= rect.x.saturating_add(rect.width).saturating_sub(12) {
            return None;
        }
        if y >= body + 42 && y < body + 96 { return Some(0); }
        if y >= body + 105 && y < body + 160 { return Some(1); }
        None
    }


    /// Hit testing for the Files window mirrors the renderer's directory list.
    /// All operations affect only a read-only in-memory directory browser.
    fn files_click(&mut self, rect: crate::window_manager::Rect, x: usize, y: usize) {
        let bar_h = if rect.height < 170 { 30 } else { 45 };
        let body = rect.y + bar_h + 8;
        let side_width = (rect.width / 4).clamp(70, 170);
        let content_x = rect.x + side_width + 27;
        if x >= rect.x + 12 && x < rect.x + 10 + side_width
            && y >= body + 6 && y < body + 38 {
            self.files.home();
            return;
        }
        if x < content_x || x >= rect.x + rect.width.saturating_sub(12) {
            return;
        }
        if y >= body + 7 && y < body + 35 {
            self.files.up();
            return;
        }
        let rows_start = body + 42;
        if y >= rows_start {
            let offset = y - rows_start;
            if offset % 47 < 40 && offset / 47 < 7 {
                self.files.open_child(offset / 47);
            }
        }
    }

    pub fn enable_pointer(&mut self) {
        if !self.pointer.visible {
            self.pointer.visible = true;
            self.changes = self.changes.wrapping_add(1);
        }
    }

    /// Map the same Dock icon layout used by the immediate renderer.
    pub fn dock_app_at(layout: Layout, x: usize, y: usize) -> Option<App> {
        let width = layout.width.min(500).saturating_sub(22);
        let dock_x = layout.width.saturating_sub(width) / 2;
        let dock_y = layout.height.saturating_sub(layout.dock_height + 10);
        let icon = layout.dock_height.saturating_sub(18).min(46);
        let spacing = icon + 14;
        let base = dock_x + width.saturating_sub(3 * spacing) / 2 + 7;
        if y < dock_y + 8 || y >= dock_y + 8 + icon {
            return None;
        }
        for (i, app) in [App::Files, App::Terminal, App::Settings]
            .iter()
            .enumerate()
        {
            let start = base + i * spacing;
            if x >= start && x < start + icon {
                return Some(*app);
            }
        }
        None
    }

    fn launcher_app_at(&self, x: usize, y: usize) -> Option<App> {
        let width = self.layout.width.saturating_sub(40).min(410);
        let start_x = self.layout.width.saturating_sub(width) / 2;
        let start_y = self.layout.top_bar + 16;
        if x < start_x || x >= start_x + width || y < start_y {
            return None;
        }
        match y - start_y {
            50..=77 => Some(App::Files),
            78..=101 => Some(App::Terminal),
            102..=131 => Some(App::Settings),
            _ => None,
        }
    }

    /// Mouse zones for the three visible desktop launcher cards.
    pub fn desktop_shortcut_at(layout: Layout, x: usize, y: usize) -> Option<App> {
        if layout.width < 500 || layout.height < 440 || x < 30 || x >= 130 {
            return None;
        }
        let start = layout.top_bar + 145;
        if y < start { return None; }
        let index = (y - start) / 78;
        if (y - start) % 78 >= 66 { return None; }
        match index {
            0 => Some(App::Files),
            1 => Some(App::Terminal),
            2 => Some(App::Settings),
            _ => None,
        }
    }

    /// The top panel menus are real desktop actions rather than mock labels.
    fn top_menu_at(&self, x: usize, y: usize) -> Option<usize> {
        if y >= self.layout.top_bar || self.layout.width < 720 { return None; }
        let mut start = 170;
        for (index, id) in [TextId::Desktop, TextId::File,
                             TextId::View, TextId::Windows].iter().enumerate() {
            let end = start + tr(self.language, *id).chars().count() * 6 + 16;
            if x >= start && x < end { return Some(index); }
            start = end;
        }
        None
    }

    /// Handle one decoded packet. The Y-axis is already screen-oriented.
    /// The pointer and all window coordinates stay inside the GUI viewport.
    pub fn handle_mouse(&mut self, packet: MousePacket) -> bool {
        if !self.pointer.visible {
            return false;
        }
        let before = (self.windows, self.pointer, self.launcher, self.dark, self.language, self.files);
        self.pointer.x = self.pointer.x
            .saturating_add_signed(packet.dx as isize)
            .min(self.layout.width.saturating_sub(1));
        self.pointer.y = self.pointer.y
            .saturating_add_signed(packet.dy as isize)
            .min(self.layout.height.saturating_sub(1));

        if packet.left && !self.pointer.left {
            let x = self.pointer.x;
            let y = self.pointer.y;
            if self.launcher {
                if let Some(app) = self.launcher_app_at(x, y) {
                    self.windows.open(app);
                }
                self.launcher = false;
            } else if let Some(app) = Self::dock_app_at(self.layout, x, y) {
                self.windows.open(app);
            } else if y < self.layout.top_bar && x < 46 {
                self.launcher = true;
            } else if let Some(menu) = self.top_menu_at(x, y) {
                match menu {
                    0 => self.launcher = !self.launcher,
                    1 => self.windows.open(App::Files),
                    2 => self.dark = !self.dark,
                    _ => self.windows.show_desktop(),
                }
            } else if let Some((app, title)) = self.windows.activate_at(x, y) {
                let rect = self.windows.rect(app);
                // The three QEMU demo traffic lights behave as close /
                // minimize / maximize. No callbacks or untrusted app code.
                if title && y >= rect.y + 13 && y < rect.y + 24 {
                    let action = (0..3).find(|i| {
                        let left = rect.x + 14 + i * 19;
                        x >= left && x < left + 11
                    });
                    match action {
                        Some(0) => self.windows.close_focused(),
                        Some(1) => self.windows.minimize_focused(),
                        Some(2) => self.windows.toggle_maximize(),
                        _ => self.pointer.dragging = Some(app),
                    }
                } else if title {
                    self.pointer.dragging = Some(app);
                } else if app == App::Settings {
                    match Self::settings_row(rect, x, y) {
                        Some(0) => self.dark = !self.dark,
                        Some(1) => self.cycle_language(),
                        _ => {}
                    }
                } else if app == App::Files {
                    self.files_click(rect, x, y);
                }
            } else if let Some(app) = Self::desktop_shortcut_at(self.layout, x, y) {
                self.windows.open(app);
            }
        } else if packet.left && self.pointer.left {
            if let Some(app) = self.pointer.dragging {
                self.windows.drag_window(app, packet.dx, packet.dy);
            }
        } else if !packet.left {
            self.pointer.dragging = None;
        }
        self.pointer.left = packet.left;
        let changed = before != (self.windows, self.pointer, self.launcher, self.dark, self.language, self.files);
        if changed {
            self.changes = self.changes.wrapping_add(1);
        }
        changed
    }

    /// Return true only when a state transition changes the visible desktop.
    pub fn handle(&mut self, key: Key) -> bool {
        let before = (self.windows, self.launcher, self.dark, self.language, self.files);
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
            Key::CycleLanguage => self.cycle_language(),
            Key::ShowDesktop => self.windows.show_desktop(),
            Key::NextApp => {
                self.launcher = false;
                self.windows.cycle();
            }
            Key::Minimize => self.windows.minimize_focused(),
            Key::Maximize => self.windows.toggle_maximize(),
            Key::Move(direction) => self.windows.translate_focused(direction),
            Key::Resize(direction) => self.windows.resize_focused(direction),
            Key::Backspace if self.active() == Some(App::Files) => { self.files.up(); },
            Key::Confirm | Key::Backspace | Key::Text(_) => {}
            Key::Escape => {
                if self.launcher {
                    self.launcher = false;
                } else {
                    self.windows.close_focused();
                }
            }
        }
        let changed = before != (self.windows, self.launcher, self.dark, self.language, self.files);
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
            0x42 => Some(Key::CycleLanguage), // F8
            0x43 => Some(Key::ShowDesktop),   // F9
            0x0f => Some(Key::NextApp),      // Tab
            0x01 => Some(Key::Escape),       // Escape
            0x1c => Some(Key::Confirm),      // Enter
            0x0e => Some(Key::Backspace),    // Backspace
            // BIOS/OVMF/QEMU translated US PS/2 scan-set-1 layout.
            // Only safe ASCII hostname characters are emitted. The host
            // layout may differ, so this is NOT a full international IME.
            0x02..=0x0b => {
                let digits = *b"1234567890";
                Some(Key::Text(digits[(code - 0x02) as usize]))
            }
            0x0c => Some(Key::Text(b'-')),
            0x10..=0x19 => {
                let letters = *b"qwertyuiop";
                Some(Key::Text(letters[(code - 0x10) as usize]))
            }
            0x1e..=0x26 => {
                let letters = *b"asdfghjkl";
                Some(Key::Text(letters[(code - 0x1e) as usize]))
            }
            0x2c..=0x32 => {
                let letters = *b"zxcvbnm";
                Some(Key::Text(letters[(code - 0x2c) as usize]))
            }
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
    fn desktop_starts_with_valid_default_preferences() {
        let s = Desktop::new();
        assert_eq!(s.hostname.as_str(), "zilla-pc");
        assert_eq!(s.language, Language::Ukrainian);
        assert_eq!(s.timezone, TimeZone::Kyiv);
    }

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
        assert_eq!(decoder.feed(0x42), Some(Key::CycleLanguage));
        assert_eq!(decoder.feed(0x43), Some(Key::ShowDesktop));
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
    fn f8_cycles_three_locales_without_losing_windows() {
        let mut state = Desktop::new();
        state.handle(Key::Terminal);
        let before = state.windows;
        assert!(state.handle(Key::CycleLanguage));
        assert_eq!(state.language, Language::English);
        assert!(state.handle(Key::CycleLanguage));
        assert_eq!(state.language, Language::Russian);
        assert!(state.handle(Key::CycleLanguage));
        assert_eq!(state.language, Language::Ukrainian);
        assert_eq!(state.windows, before);
    }

    #[test]
    fn settings_click_changes_theme_and_language() {
        let mut state = Desktop::new();
        state.enable_pointer();
        state.handle(Key::Settings);
        let rect = state.windows.rect(App::Settings);
        let side = (rect.width / 4).clamp(70, 170);
        state.pointer.x = rect.x + side + 40;
        state.pointer.y = rect.y + 45 + 8 + 62;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert!(!state.dark);
        state.handle_mouse(test_packet(0, 0, false));
        state.pointer.y = rect.y + 45 + 8 + 128;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(state.language, Language::English);
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

    fn test_packet(dx: i16, dy: i16, left: bool) -> MousePacket {
        MousePacket { dx, dy, left, right: false, middle: false }
    }

    #[test]
    fn pointer_is_clamped_to_desktop_bounds() {
        let mut state = Desktop::new();
        state.enable_pointer();
        for _ in 0..20 {
            state.handle_mouse(test_packet(255, 255, false));
        }
        assert_eq!((state.pointer.x, state.pointer.y), (1023, 767));
        for _ in 0..20 {
            state.handle_mouse(test_packet(-255, -255, false));
        }
        assert_eq!((state.pointer.x, state.pointer.y), (0, 0));
    }

    #[test]
    fn desktop_shortcuts_and_top_menu_are_clickable() {
        let mut state = Desktop::new();
        state.enable_pointer();
        assert_eq!(Desktop::desktop_shortcut_at(state.layout, 50,
            state.layout.top_bar + 145 + 78 + 10), Some(App::Terminal));
        state.pointer.x = 50;
        state.pointer.y = state.layout.top_bar + 145 + 78 + 10;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(state.active(), Some(App::Terminal));
        state.handle_mouse(test_packet(0, 0, false));
        state.pointer.x = 170;
        state.pointer.y = 19;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert!(state.launcher);
    }

    #[test]
    fn clicking_dock_opens_second_window() {
        let mut state = Desktop::new();
        state.enable_pointer();
        let layout = Layout::for_size(1024, 768).unwrap();
        let dock_icon_y = layout.height - layout.dock_height - 2;
        let dock_icon_x = (0..1024)
            .find(|x| Desktop::dock_app_at(layout, *x, dock_icon_y)
                == Some(App::Terminal)).unwrap();
        state.pointer.x = dock_icon_x;
        state.pointer.y = dock_icon_y;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(state.active(), Some(App::Terminal));
        assert_eq!(state.windows.visible_count(), 2);
        state.handle_mouse(test_packet(0, 0, false));
    }

    #[test]
    fn title_drag_moves_window_and_close_button_hides_it() {
        let mut state = Desktop::new();
        state.enable_pointer();
        let rect = state.windows.rect(App::Files);
        state.pointer.x = rect.x + 110;
        state.pointer.y = rect.y + 18;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(state.pointer.dragging(), Some(App::Files));
        assert!(state.handle_mouse(test_packet(15, 10, true)));
        assert!(state.windows.rect(App::Files).x >= rect.x + 15);
        state.handle_mouse(test_packet(0, 0, false));
        let moved = state.windows.rect(App::Files);
        state.pointer.x = moved.x + 17;
        state.pointer.y = moved.y + 16;
        state.handle_mouse(test_packet(0, 0, true));
        assert_eq!(state.active(), None);
    }

    #[test]
    fn launcher_mouse_selection_activates_settings() {
        let mut state = Desktop::new();
        state.enable_pointer();
        state.handle(Key::Launcher);
        state.pointer.x = 400;
        state.pointer.y = 42 + 16 + 110;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(state.active(), Some(App::Settings));
        assert!(!state.launcher);
    }

    #[test]
    fn files_window_opens_real_in_memory_directories_and_previews_file() {
        let mut state = Desktop::new();
        state.enable_pointer();
        let rect = state.windows.rect(App::Files);
        let body = rect.y + 45 + 8;
        let cx = rect.x + (rect.width / 4).clamp(70, 170) + 40;
        let mut click_row = |state: &mut Desktop, row: usize| {
            state.pointer.x = cx;
            state.pointer.y = body + 42 + row * 47 + 12;
            assert!(state.handle_mouse(test_packet(0, 0, true)));
            state.handle_mouse(test_packet(0, 0, false));
        };
        click_row(&mut state, 1); // /Users
        click_row(&mut state, 0); // /Users/Guest
        click_row(&mut state, 1); // /Users/Guest/Documents
        click_row(&mut state, 0); // WELCOME.TXT
        assert_eq!(crate::fs::node(state.files.current).unwrap().path,
                   "/Users/Guest/Documents");
        assert!(state.files.preview().is_some());
        assert!(state.handle(Key::Backspace));
        assert!(state.files.preview().is_none());
        assert!(state.handle(Key::Backspace));
        assert_eq!(crate::fs::node(state.files.current).unwrap().path,
                   "/Users/Guest");
    }

    #[test]
    fn setup_input_scancodes_decode_ascii_and_enter() {
        let mut d = ScancodeDecoder::new();
        assert_eq!(d.feed(0x1c), Some(Key::Confirm));
        assert_eq!(d.feed(0x0e), Some(Key::Backspace));
        assert_eq!(d.feed(0x10), Some(Key::Text(b'q')));
        assert_eq!(d.feed(0x26), Some(Key::Text(b'l')));
        assert_eq!(d.feed(0x32), Some(Key::Text(b'm')));
        assert_eq!(d.feed(0x0c), Some(Key::Text(b'-')));
        assert_eq!(d.feed(0x02), Some(Key::Text(b'1')));
        assert_eq!(d.feed(0x82), None);
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
