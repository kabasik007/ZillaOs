//! Zilla Desktop event state and small translated PS/2 keyboard decoder.
//! Real processes and IPC-backed application surfaces are a later milestone.

use crate::mouse::MousePacket;
use crate::fs::{self, Browser};
use crate::fs::ramfs::{RamFs, DOCUMENTS_DIR};
use crate::apps::notepad::Notepad;
use crate::apps::notepad_ui::{editor_action, EditorAction};
use crate::shell::clock::{ClockTime, ClockDate};
use crate::shell::desktop_icons::DesktopIcons;
use crate::video::VideoAdapter;
use crate::shell::{start_menu, taskbar, task_view};
use crate::shell::personalization::{self, Accent, Wallpaper, SettingsPage, SettingsAction, Menu, MenuAction};
use crate::shell::theme_profile::{ThemeProfile, ProfileStatus, PROFILE_FILE};
use crate::locale::{text as tr, TextId};
use crate::setup::{DeviceName, Language, Region, TimeZone};
use crate::window_manager::{Move, WindowManager};

pub use crate::apps::App;
use crate::services::app_manager::{ApplicationManager, LaunchTarget};
use crate::apps::app_center;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Launcher,
    Files,
    Terminal,
    Settings,
    AppCenter,
    Notepad,
    TaskManager,
    SaveDocument,
    NewDocument,
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
    pub ramfs: RamFs,
    pub notepad: Notepad,
    pub icons: DesktopIcons,
    pub clock: Option<ClockTime>,
    pub date: Option<ClockDate>,
    pub video: VideoAdapter,
    layout: Layout,
    pub launcher: bool,
    pub launcher_step: u8,
    pub dark: bool,
    /// Volatile desktop appearance and context menu (no disk persistence).
    pub wallpaper: Wallpaper,
    /// Set once from a validated Limine boot module, never from PCI hardware.
    pub custom_wallpaper_available: bool,
    pub accent: Accent,
    /// Status of THEME.TXT in volatile Documents RAMFS.
    pub profile_status: ProfileStatus,
    pub settings_page: SettingsPage,
    pub context_menu: Option<Menu>,
    right_down: bool,
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
            ramfs: RamFs::new(),
            notepad: Notepad::new(),
            icons: DesktopIcons::new(layout),
            clock: None,
            date: None,
            video: VideoAdapter::FirmwareOnly,
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
            launcher_step: 0,
            dark: true,
            wallpaper: Wallpaper::Ocean,
            custom_wallpaper_available: false,
            accent: Accent::Teal,
            profile_status: ProfileStatus::SessionOnly,
            settings_page: SettingsPage::Overview,
            context_menu: None,
            right_down: false,
            changes: 0,
        }
    }

    pub const fn active(&self) -> Option<App> {
        self.windows.focused()
    }
    pub fn launch_app(&mut self, app: App) {
        if app == App::Settings {
            self.settings_page = SettingsPage::Overview;
        }
        self.context_menu = None;
        ApplicationManager::launch(&mut self.windows, LaunchTarget::Builtin(app))
            .expect("installed built-in application");
    }

    /// Read-only RTC update. The caller triggers repaint only for changed
    /// seconds; invalid CMOS readings do not erase the last valid time.
    /// Advance the slide-out panel one frame. Called on a separate throttle
    /// from keyboard/mouse events; the kernel has no scheduler at this stage.
    pub fn tick_launcher(&mut self) -> bool {
        let old = self.launcher_step;
        if self.launcher {
            self.launcher_step = self.launcher_step.saturating_add(1).min(start_menu::STEPS);
        } else {
            self.launcher_step = self.launcher_step.saturating_sub(1);
        }
        if self.launcher_step != old {
            self.changes = self.changes.wrapping_add(1);
            true
        } else { false }
    }

    /// CMOS RTC calendar date; no timezone conversion is claimed.
    pub fn update_date(&mut self, date: Option<ClockDate>) -> bool {
        if let Some(date) = date {
            if self.date != Some(date) {
                self.date = Some(date);
                self.changes = self.changes.wrapping_add(1);
                return true;
            }
        }
        false
    }

    pub fn update_clock(&mut self, time: Option<ClockTime>) -> bool {
        if let Some(time) = time {
            if self.clock != Some(time) {
                self.clock = Some(time);
                self.changes = self.changes.wrapping_add(1);
                return true;
            }
        }
        false
    }

    pub fn cycle_language(&mut self) {
        self.language = match self.language {
            Language::Ukrainian => Language::English,
            Language::English => Language::Russian,
            Language::Russian => Language::Ukrainian,
        };
    }

    /// Right-click only on unused wallpaper, never over a window, dock,
    /// taskbar, launcher, shortcuts or draggable desktop folder icons.
    fn can_open_desktop_menu(&self, x: usize, y: usize) -> bool {
        if y < self.layout.top_bar
            || y >= self.layout.height.saturating_sub(self.layout.dock_height + 12)
            || self.launcher
            || self.launcher_step != 0
            || Self::desktop_shortcut_at(self.layout, x, y).is_some()
            || self.icons.hit(x, y).is_some()
            || Self::dock_app_at(self.layout, x, y).is_some()
        {
            return false;
        }
        !self.windows.back_to_front().iter().copied().any(|app| {
            self.windows.window(app).visible()
                && WindowManager::contains(self.windows.rect(app), x, y)
        })
    }

    fn save_theme_profile(&mut self) {
        let profile = ThemeProfile {
            wallpaper: self.wallpaper,
            accent: self.accent,
            dark: self.dark,
        };
        let (data, len) = profile.encode();
        self.profile_status = if self.ramfs.save(PROFILE_FILE, &data[..len]).is_ok() {
            ProfileStatus::Saved
        } else {
            ProfileStatus::StorageFull
        };
    }

    fn load_theme_profile(&mut self) {
        let parsed = self.ramfs.find(PROFILE_FILE)
            .and_then(|index| self.ramfs.get(index))
            .map(|file| ThemeProfile::decode(file.content()));
        match parsed {
            None => self.profile_status = ProfileStatus::NotFound,
            Some(Err(_)) => self.profile_status = ProfileStatus::InvalidFile,
            Some(Ok(profile)) => {
                // Custom profiles require a valid boot-time image asset.
                if profile.wallpaper == Wallpaper::Custom
                    && !self.custom_wallpaper_available
                {
                    self.profile_status = ProfileStatus::ImageUnavailable;
                    return;
                }
                // Atomic application: first validate all fields, only then
                // change visible desktop state.
                self.wallpaper = profile.wallpaper;
                self.accent = profile.accent;
                self.dark = profile.dark;
                self.profile_status = ProfileStatus::Loaded;
            }
        }
    }

    fn settings_action(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::ToggleTheme => {
                self.dark = !self.dark;
                self.profile_status = ProfileStatus::Modified;
            }
            SettingsAction::CycleLanguage => self.cycle_language(),
            SettingsAction::OpenPersonalization => {
                self.settings_page = SettingsPage::Personalization;
            }
            SettingsAction::Back => self.settings_page = SettingsPage::Overview,
            SettingsAction::CycleWallpaper => {
                self.wallpaper = self.wallpaper.next();
                self.profile_status = ProfileStatus::Modified;
            }
            SettingsAction::CycleAccent => {
                self.accent = self.accent.next();
                self.profile_status = ProfileStatus::Modified;
            }
            SettingsAction::SetWallpaper(wallpaper) => {
                if wallpaper == Wallpaper::Custom && !self.custom_wallpaper_available {
                    self.profile_status = ProfileStatus::ImageUnavailable;
                } else {
                    self.wallpaper = wallpaper;
                    self.profile_status = ProfileStatus::Modified;
                }
            }
            SettingsAction::SetAccent(accent) => {
                self.accent = accent;
                self.profile_status = ProfileStatus::Modified;
            }
            SettingsAction::SaveProfile => self.save_theme_profile(),
            SettingsAction::LoadProfile => self.load_theme_profile(),
            SettingsAction::ResetProfile => {
                let defaults = ThemeProfile::default();
                self.wallpaper = defaults.wallpaper;
                self.accent = defaults.accent;
                self.dark = defaults.dark;
                self.profile_status = ProfileStatus::Modified;
            }
        }
    }

    /// All Files hit testing lives in the shell module; browser state owns
    /// only virtual in-memory paths, never host or block-device operations.
    fn files_click(&mut self, rect: crate::window_manager::Rect, x: usize, y: usize) {
        use crate::shell::files_interaction::{files_action, FilesAction};
        match files_action(rect, x, y) {
            Some(FilesAction::Home) => { self.files.home(); }
            Some(FilesAction::Up) => { self.files.up(); }
            Some(FilesAction::OpenRow(index)) => {
                let static_count = fs::child_count(self.files.current);
                if self.files.current == DOCUMENTS_DIR && index >= static_count {
                    if let Some((_, file)) = self.ramfs.nth(index - static_count) {
                        let _ = self.notepad.open(file.name(), file.content());
                        self.launch_app(App::Notepad);
                    }
                } else {
                    self.files.open_child(index);
                }
            }
            None => {}
        }
    }

    fn notepad_click(&mut self, rect: crate::window_manager::Rect,
                     x: usize, y: usize) {
        match editor_action(rect, x, y) {
            Some(EditorAction::New) => { self.notepad.new_document(); }
            Some(EditorAction::Save) => { let _ = self.notepad.save(&mut self.ramfs); }
            Some(EditorAction::Documents) => {
                self.files.current = DOCUMENTS_DIR;
                self.files.preview_file = None;
                self.launch_app(App::Files);
            }
            None => {}
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
        let base = dock_x + width.saturating_sub(App::COUNT * spacing) / 2 + 7;
        if y < dock_y + 8 || y >= dock_y + 8 + icon {
            return None;
        }
        for (i, app) in App::ALL.iter().enumerate() {
            let start = base + i * spacing;
            if x >= start && x < start + icon {
                return Some(*app);
            }
        }
        None
    }

    fn launcher_app_at(&self, x: usize, y: usize) -> Option<App> {
        start_menu::app_row(self.layout, self.launcher_step, x, y)
            .and_then(|row| App::ALL.get(row).copied())
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
        App::ALL.get(index).copied()
    }

    /// The top panel menus are real desktop actions rather than mock labels.
    fn top_menu_at(&self, x: usize, y: usize) -> Option<usize> {
        taskbar::menu_hit(self.layout, self.language, x, y)
    }

    fn toggle_task_manager(&mut self) {
        if self.active() == Some(App::TaskManager) {
            self.windows.minimize_focused();
        } else {
            self.launch_app(App::TaskManager);
        }
        self.launcher = false;
    }

    /// Handle one decoded packet. The Y-axis is already screen-oriented.
    /// The pointer and all window coordinates stay inside the GUI viewport.
    pub fn handle_mouse(&mut self, packet: MousePacket) -> bool {
        if !self.pointer.visible {
            return false;
        }
        let before = (self.windows, self.pointer, self.launcher, self.dark, self.language, self.files, self.icons,
                      self.context_menu, self.settings_page, self.wallpaper, self.accent, self.profile_status);
        let old_note = self.notepad.revision;
        let old_ramfs = self.ramfs.revision;
        self.pointer.x = self.pointer.x
            .saturating_add_signed(packet.dx as isize)
            .min(self.layout.width.saturating_sub(1));
        self.pointer.y = self.pointer.y
            .saturating_add_signed(packet.dy as isize)
            .min(self.layout.height.saturating_sub(1));

        // Trigger once on the right-button press edge, not on every motion
        // packet while the button is held.
        if packet.right && !self.right_down {
            let x = self.pointer.x;
            let y = self.pointer.y;
            if self.can_open_desktop_menu(x, y) {
                self.context_menu = Some(Menu::at(self.layout, x, y));
            } else {
                self.context_menu = None;
                if self.launcher { self.launcher = false; }
            }
        }
        self.right_down = packet.right;

        if packet.left && !self.pointer.left {
            let x = self.pointer.x;
            let y = self.pointer.y;
            if let Some(menu) = self.context_menu.take() {
                // The click always belongs to the popup, including outside
                // the popup; never activate a window underneath it.
                match menu.hit(x, y) {
                    Some(MenuAction::Personalization) => {
                        self.launch_app(App::Settings);
                        self.settings_page = SettingsPage::Personalization;
                    }
                    Some(MenuAction::DisplaySettings) => {
                        self.launch_app(App::Settings);
                        self.settings_page = SettingsPage::Overview;
                    }
                    None => {}
                }
            } else if self.launcher {
                if let Some(app) = self.launcher_app_at(x, y) {
                    self.launch_app(app);
                }
                self.launcher = false;
            } else if let Some(action) = taskbar::hit(self.layout, x, y) {
                match action {
                    taskbar::Action::Start => self.launcher = !self.launcher,
                    taskbar::Action::Tasks => self.toggle_task_manager(),
                }
            } else if let Some(app) = Self::dock_app_at(self.layout, x, y) {
                self.launch_app(app);
            } else if y < self.layout.top_bar && x < 46 {
                self.launcher = true;
            } else if let Some(menu) = self.top_menu_at(x, y) {
                match menu {
                    0 => self.launcher = !self.launcher,
                    1 => self.launch_app(App::Files),
                    2 => {
                        self.dark = !self.dark;
                        self.profile_status = ProfileStatus::Modified;
                    },
                    3 => self.windows.show_desktop(),
                    _ => self.toggle_task_manager(),
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
                    if let Some(action) = personalization::settings_hit(
                        rect, self.settings_page, x, y,
                    ) {
                        self.settings_action(action);
                    }
                } else if app == App::Files {
                    self.files_click(rect, x, y);
                } else if app == App::AppCenter {
                    if let Some(chosen) = app_center::app_row_at(rect, x, y) {
                        self.launch_app(chosen);
                    }
                } else if app == App::Notepad {
                    self.notepad_click(rect, x, y);
                } else if app == App::TaskManager {
                    if let Some(chosen) = task_view::row_at(rect, x, y) {
                        if chosen != App::TaskManager { self.launch_app(chosen); }
                    }
                }
            } else if let Some(app) = Self::desktop_shortcut_at(self.layout, x, y) {
                self.launch_app(app);
            } else if let Some(icon) = self.icons.hit(x, y) {
                self.icons.begin(icon);
            }
        } else if packet.left && self.pointer.left {
            if let Some(app) = self.pointer.dragging {
                self.windows.drag_window(app, packet.dx, packet.dy);
            } else if self.icons.dragging.is_some() {
                self.icons.move_by(packet.dx, packet.dy);
            }
        } else if !packet.left {
            self.pointer.dragging = None;
            if let Some(directory) = self.icons.finish() {
                self.files.current = directory;
                self.files.preview_file = None;
                self.launch_app(App::Files);
            }
        }
        self.pointer.left = packet.left;
        let changed = before != (self.windows, self.pointer, self.launcher, self.dark, self.language, self.files, self.icons,
                                  self.context_menu, self.settings_page, self.wallpaper, self.accent, self.profile_status)
            || old_note != self.notepad.revision || old_ramfs != self.ramfs.revision;
        if changed {
            self.changes = self.changes.wrapping_add(1);
        }
        changed
    }

    /// Return true only when a state transition changes the visible desktop.
    pub fn handle(&mut self, key: Key) -> bool {
        let before = (self.windows, self.launcher, self.dark, self.language, self.files,
                      self.context_menu, self.settings_page, self.profile_status);
        let old_note = self.notepad.revision;
        let old_ramfs = self.ramfs.revision;
        match key {
            Key::Launcher => self.launcher = !self.launcher,
            Key::TaskManager => self.toggle_task_manager(),
            Key::Files | Key::Terminal | Key::Settings | Key::AppCenter | Key::Notepad => {
                let app = match key {
                    Key::Files => App::Files,
                    Key::Terminal => App::Terminal,
                    Key::Settings => App::Settings,
                    Key::AppCenter => App::AppCenter,
                    Key::Notepad => App::Notepad,
                    _ => unreachable!(),
                };
                self.launch_app(app);
                self.launcher = false;
            }
            Key::ToggleTheme => {
                self.dark = !self.dark;
                self.profile_status = ProfileStatus::Modified;
            },
            Key::CycleLanguage => self.cycle_language(),
            Key::ShowDesktop => self.windows.show_desktop(),
            Key::NextApp => {
                self.launcher = false;
                self.windows.cycle();
            }
            Key::Minimize => self.windows.minimize_focused(),
            Key::Maximize => self.windows.toggle_maximize(),
            Key::Move(direction) if self.active() == Some(App::Notepad) => {
                match direction {
                    Move::Left => { self.notepad.move_left(); }
                    Move::Right => { self.notepad.move_right(); }
                    _ => {}
                }
            }
            Key::Move(direction) => self.windows.translate_focused(direction),
            Key::Resize(direction) => self.windows.resize_focused(direction),
            Key::SaveDocument if self.active() == Some(App::Notepad) => {
                let _ = self.notepad.save(&mut self.ramfs);
            }
            Key::NewDocument if self.active() == Some(App::Notepad) => {
                self.notepad.new_document();
            }
            Key::Text(ch) if self.active() == Some(App::Notepad) => {
                self.notepad.insert(ch);
            }
            Key::Confirm if self.active() == Some(App::Notepad) => {
                self.notepad.insert(b'\n');
            }
            Key::Backspace if self.active() == Some(App::Notepad) => {
                self.notepad.backspace();
            }
            Key::Backspace if self.active() == Some(App::Files) => { self.files.up(); }
            Key::SaveDocument | Key::NewDocument | Key::Confirm |
            Key::Backspace | Key::Text(_) => {}
            Key::Escape => {
                if self.context_menu.take().is_some() {
                    // Esc dismisses context menu without closing an app.
                } else if self.launcher {
                    self.launcher = false;
                } else if self.active() == Some(App::Settings)
                    && self.settings_page == SettingsPage::Personalization
                {
                    self.settings_page = SettingsPage::Overview;
                } else {
                    self.windows.close_focused();
                }
            }
        }
        let changed = before != (self.windows, self.launcher, self.dark, self.language, self.files,
                                  self.context_menu, self.settings_page, self.profile_status)
            || old_note != self.notepad.revision || old_ramfs != self.ramfs.revision;
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
    ctrl: bool,
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
            ctrl: false,
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
        if code == 0x1d {
            self.ctrl = true;
            self.extended = false;
            return None;
        }
        if code == 0x9d {
            self.ctrl = false;
            self.extended = false;
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

        if self.ctrl && self.shift && code == 0x01 {
            return Some(Key::TaskManager); // Ctrl+Shift+Esc: real taskbar action
        }
        if self.ctrl && code == 0x1f { return Some(Key::SaveDocument); } // Ctrl+S
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
            0x44 => Some(Key::AppCenter),      // F10
            0x57 => Some(Key::SaveDocument),  // F11
            0x58 => Some(Key::NewDocument),   // F12
            0x0f => Some(Key::NextApp),      // Tab
            0x01 => Some(Key::Escape),       // Escape
            0x1c => Some(Key::Confirm),      // Enter
            0x0e => Some(Key::Backspace),    // Backspace
            // BIOS/OVMF/QEMU translated US PS/2 scan-set-1 layout.
            // Only safe ASCII hostname characters are emitted. The host
            // layout may differ, so this is NOT a full international IME.
            0x02..=0x0b => {
                let digits = *b"1234567890";
                let shifted = *b"!@#$%^&*()";
                Some(Key::Text(if self.shift { shifted[(code - 0x02) as usize] }
                               else { digits[(code - 0x02) as usize] }))
            }
            0x0c => Some(Key::Text(if self.shift { b'_' } else { b'-' })),
            0x0d => Some(Key::Text(if self.shift { b'+' } else { b'=' })),
            0x39 => Some(Key::Text(b' ')),
            0x1a => Some(Key::Text(if self.shift { b'{' } else { b'[' })),
            0x1b => Some(Key::Text(if self.shift { b'}' } else { b']' })),
            0x27 => Some(Key::Text(if self.shift { b':' } else { b';' })),
            0x28 => Some(Key::Text(if self.shift { b'"' } else { b'\'' })),
            0x29 => Some(Key::Text(if self.shift { b'~' } else { b'`' })),
            0x2b => Some(Key::Text(if self.shift { b'|' } else { b'\\' })),
            0x33 => Some(Key::Text(if self.shift { b'<' } else { b',' })),
            0x34 => Some(Key::Text(if self.shift { b'>' } else { b'.' })),
            0x35 => Some(Key::Text(if self.shift { b'?' } else { b'/' })),
            0x10..=0x19 => {
                let letters = *b"qwertyuiop";
                Some(Key::Text(if self.shift { letters[(code - 0x10) as usize].to_ascii_uppercase() } else { letters[(code - 0x10) as usize] }))
            }
            0x1e..=0x26 => {
                let letters = *b"asdfghjkl";
                Some(Key::Text(if self.shift { letters[(code - 0x1e) as usize].to_ascii_uppercase() } else { letters[(code - 0x1e) as usize] }))
            }
            0x2c..=0x32 => {
                let letters = *b"zxcvbnm";
                Some(Key::Text(if self.shift { letters[(code - 0x2c) as usize].to_ascii_uppercase() } else { letters[(code - 0x2c) as usize] }))
            }
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.extended = false;
        self.shift = false;
        self.ctrl = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_manager::WindowMode;

    #[test]
    fn rtc_seconds_only_redraw_when_clock_value_changes() {
        let mut desktop = Desktop::new();
        assert!(desktop.update_clock(ClockTime::new(11, 22, 33)));
        assert_eq!(desktop.clock, ClockTime::new(11, 22, 33));
        assert!(!desktop.update_clock(ClockTime::new(11, 22, 33)));
        assert!(!desktop.update_clock(None));
        assert!(desktop.update_clock(ClockTime::new(11, 22, 34)));
    }

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
        assert_eq!(decoder.feed(0x44), Some(Key::AppCenter));
        assert_eq!(decoder.feed(0x57), Some(Key::SaveDocument));
        assert_eq!(decoder.feed(0x58), Some(Key::NewDocument));
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
    fn taskbar_start_and_task_manager_are_real_window_actions() {
        let mut desktop = Desktop::new();
        desktop.enable_pointer();
        let task_button = taskbar::button(desktop.layout, taskbar::Action::Tasks).unwrap();
        desktop.pointer.x = task_button.x + 12;
        desktop.pointer.y = task_button.y + 12;
        assert!(desktop.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(desktop.active(), Some(App::TaskManager));
        desktop.handle_mouse(test_packet(0, 0, false));
        assert!(desktop.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(desktop.windows.window(App::TaskManager).mode, WindowMode::Minimized);
        desktop.handle_mouse(test_packet(0, 0, false));
        assert!(desktop.handle(Key::TaskManager));
        assert_eq!(desktop.active(), Some(App::TaskManager));
        let rect = desktop.windows.rect(App::TaskManager);
        let cx = rect.x + (rect.width / 4).clamp(70,170) + 27;
        let body = rect.y + 45 + 8;
        desktop.pointer.x = cx + 12;
        desktop.pointer.y = body + task_view::ROW_START + 10;
        assert!(desktop.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(desktop.active(), Some(App::Files));
    }

    #[test]
    fn ctrl_shift_escape_opens_task_manager_without_closing_other_windows() {
        let mut keyboard=ScancodeDecoder::new();
        assert_eq!(keyboard.feed(0x1d),None);
        assert_eq!(keyboard.feed(0x2a),None);
        assert_eq!(keyboard.feed(0x01),Some(Key::TaskManager));
        keyboard.feed(0xaa);
        keyboard.feed(0x9d);
        assert_eq!(keyboard.feed(0x01),Some(Key::Escape));
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
    fn notepad_keyboard_saves_and_files_reopen_edited_document() {
        let mut state = Desktop::new();
        assert!(state.handle(Key::Notepad));
        assert!(state.handle(Key::Text(b'H')));
        assert!(state.handle(Key::Text(b'i')));
        assert!(state.handle(Key::Confirm));
        assert!(state.handle(Key::SaveDocument));
        assert_eq!(state.ramfs.len(), 1);
        assert_eq!(state.ramfs.nth(0).unwrap().1.content(), b"Hi\n");
        state.handle(Key::NewDocument);
        assert_eq!(state.notepad.filename(), "NOTE02.TXT");
        state.files.current = DOCUMENTS_DIR;
        state.enable_pointer();
        let rect = state.windows.rect(App::Files);
        state.launch_app(App::Files);
        let cx = rect.x + (rect.width / 4).clamp(70, 170) + 38;
        state.pointer.x = cx;
        state.pointer.y = rect.y + 45 + 8 + 42 + 47 + 12;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(state.active(), Some(App::Notepad));
        assert_eq!(state.notepad.content(), "Hi\n");
        assert_eq!(state.notepad.filename(), "NOTE01.TXT");
    }

    #[test]
    fn ps2_typing_supports_spaces_shift_punctuation_and_ctrl_s() {
        let mut scan = ScancodeDecoder::new();
        assert_eq!(scan.feed(0x39), Some(Key::Text(b' ')));
        assert_eq!(scan.feed(0x34), Some(Key::Text(b'.')));
        assert_eq!(scan.feed(0x2a), None);
        assert_eq!(scan.feed(0x1e), Some(Key::Text(b'A')));
        assert_eq!(scan.feed(0xaa), None);
        assert_eq!(scan.feed(0x1d), None);
        assert_eq!(scan.feed(0x1f), Some(Key::SaveDocument));
        assert_eq!(scan.feed(0x9d), None);
    }

    #[test]
    fn app_center_launches_terminal_through_application_manager() {
        let mut desktop = Desktop::new();
        assert!(desktop.handle(Key::AppCenter));
        assert_eq!(desktop.active(), Some(App::AppCenter));
        desktop.enable_pointer();
        let rect = desktop.windows.rect(App::AppCenter);
        let side = (rect.width / 4).clamp(70, 170);
        desktop.pointer.x = rect.x + side + 38;
        desktop.pointer.y = rect.y + 45 + 8 + 47 + 43 + 5;
        assert!(desktop.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(desktop.active(), Some(App::Terminal));
        assert_eq!(desktop.windows.visible_count(), 3);
    }

    #[test]
    fn launcher_animates_in_and_out_without_corrupting_windows() {
        let mut desktop = Desktop::new();
        assert!(desktop.handle(Key::Launcher));
        for _ in 0..start_menu::STEPS { assert!(desktop.tick_launcher()); }
        assert!(!desktop.tick_launcher());
        assert_eq!(desktop.launcher_step, start_menu::STEPS);
        assert!(desktop.handle(Key::Launcher));
        for _ in 0..start_menu::STEPS { assert!(desktop.tick_launcher()); }
        assert_eq!(desktop.launcher_step, 0);
        assert_eq!(desktop.active(), Some(App::Files));
    }

    #[test]
    fn desktop_folder_click_opens_guest_documents_not_host_files() {
        let mut desktop = Desktop::new();
        desktop.enable_pointer();
        assert!(desktop.handle(Key::ShowDesktop));
        let at = desktop.icons.positions[0];
        desktop.pointer.x = at.x + 10;
        desktop.pointer.y = at.y + 12;
        desktop.handle_mouse(test_packet(0, 0, true));
        desktop.handle_mouse(test_packet(0, 0, false));
        assert_eq!(desktop.active(), Some(App::Files));
        assert_eq!(crate::fs::node(desktop.files.current).unwrap().path,
                   "/Users/Guest/Documents");
        desktop.handle(Key::ShowDesktop);
        let old = desktop.icons.positions[0];
        desktop.pointer.x = old.x + 8;
        desktop.pointer.y = old.y + 12;
        desktop.handle_mouse(test_packet(0, 0, true));
        desktop.handle_mouse(test_packet(-30, 12, true));
        desktop.handle_mouse(test_packet(0, 0, false));
        assert_eq!(desktop.icons.positions[0].x, old.x - 30);
        assert_eq!(desktop.active(), None);
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
        for _ in 0..start_menu::STEPS { state.tick_launcher(); }
        let rect = start_menu::panel(state.layout, state.launcher_step);
        state.pointer.x = rect.x + 50;
        state.pointer.y = rect.y + 110;
        assert!(state.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(state.active(), Some(App::Settings));
        assert!(!state.launcher);
        for _ in 0..start_menu::STEPS { state.tick_launcher(); }
        assert_eq!(state.launcher_step, 0);
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


    fn right_packet(right: bool) -> MousePacket {
        MousePacket { dx: 0, dy: 0, left: false, right, middle: false }
    }

    #[test]
    fn desktop_right_click_opens_menu_only_on_empty_wallpaper() {
        let mut d = Desktop::new();
        d.enable_pointer();
        d.pointer.x = 720;
        d.pointer.y = 600;
        assert!(d.handle_mouse(right_packet(true)));
        let menu = d.context_menu.expect("empty desktop menu");
        assert!(menu.contains(d.pointer.x, d.pointer.y));
        assert!(!d.handle_mouse(right_packet(true))); // hold must not repeat
        assert!(!d.handle_mouse(right_packet(false)));
        d.pointer.x = d.windows.rect(App::Files).x + 15;
        d.pointer.y = d.windows.rect(App::Files).y + 25;
        assert!(d.handle_mouse(right_packet(true))); // dismiss menu over window
        assert!(d.context_menu.is_none());
        d.handle_mouse(right_packet(false));
    }

    #[test]
    fn context_personalization_opens_settings_without_dismissing_other_apps() {
        let mut d = Desktop::new();
        d.enable_pointer();
        d.pointer.x = 800;
        d.pointer.y = 600;
        assert!(d.handle_mouse(right_packet(true)));
        let menu = d.context_menu.unwrap();
        d.handle_mouse(right_packet(false));
        d.pointer.x = menu.x + 18;
        d.pointer.y = menu.y + 18;
        assert!(d.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(d.active(), Some(App::Settings));
        assert_eq!(d.settings_page, SettingsPage::Personalization);
        assert!(d.context_menu.is_none());
        assert!(d.windows.window(App::Files).visible());
    }

    #[test]
    fn personalization_controls_change_wallpaper_accent_and_theme() {
        let mut d = Desktop::new();
        d.enable_pointer();
        d.launch_app(App::Settings);
        d.settings_page = SettingsPage::Personalization;
        let rect = d.windows.rect(App::Settings);
        let cx = rect.x + (rect.width / 4).clamp(70,170) + 40;
        let body = rect.y + 45 + 8;
        d.pointer.x = cx;
        d.pointer.y = body + 65;
        assert!(d.handle_mouse(test_packet(0,0,true)));
        assert_eq!(d.wallpaper, Wallpaper::Sunset);
        d.handle_mouse(test_packet(0,0,false));
        d.pointer.y = body + 130;
        assert!(d.handle_mouse(test_packet(0,0,true)));
        assert_eq!(d.accent, Accent::Violet);
        d.handle_mouse(test_packet(0,0,false));
        d.pointer.y = body + 190;
        assert!(d.handle_mouse(test_packet(0,0,true)));
        assert!(!d.dark);
        assert_eq!(d.settings_page, SettingsPage::Personalization);
    }

    #[test]
    fn escape_closes_menu_then_personalization_and_preserves_app_state() {
        let mut d = Desktop::new();
        d.enable_pointer();
        d.launch_app(App::Settings);
        d.settings_page = SettingsPage::Personalization;
        d.context_menu = Some(Menu::at(d.layout, 800, 510));
        assert!(d.handle(Key::Escape));
        assert!(d.context_menu.is_none());
        assert_eq!(d.settings_page, SettingsPage::Personalization);
        assert!(d.handle(Key::Escape));
        assert_eq!(d.settings_page, SettingsPage::Overview);
        assert_eq!(d.active(), Some(App::Settings));
    }

    #[test]
    fn outside_menu_click_dismisses_without_desktop_shortcut_activation() {
        let mut d = Desktop::new();
        d.enable_pointer();
        d.pointer.x = 800;
        d.pointer.y = 600;
        d.handle_mouse(right_packet(true));
        d.handle_mouse(right_packet(false));
        d.pointer.x = 40;
        d.pointer.y = d.layout.top_bar + 150;
        assert!(d.handle_mouse(test_packet(0,0,true)));
        assert!(d.context_menu.is_none());
        assert_eq!(d.active(), Some(App::Files)); // initial focus, no new app
    }


    #[test]
    fn theme_profile_settings_actions_save_load_reset_in_ramfs() {
        let mut d = Desktop::new();
        d.enable_pointer();
        d.launch_app(App::Settings);
        d.settings_page = SettingsPage::Personalization;
        let rect = d.windows.rect(App::Settings);
        d.settings_action(SettingsAction::SetWallpaper(Wallpaper::Forest));
        d.settings_action(SettingsAction::SetAccent(Accent::Amber));
        d.settings_action(SettingsAction::ToggleTheme);
        let profile = ThemeProfile {
            wallpaper: Wallpaper::Forest,
            accent: Accent::Amber,
            dark: false,
        };
        let save = personalization::profile_button(
            rect, personalization::ProfileButton::Save).unwrap();
        d.pointer.x = save.x + 8;
        d.pointer.y = save.y + 8;
        assert!(d.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(d.profile_status, ProfileStatus::Saved);
        assert!(d.ramfs.find(PROFILE_FILE).is_some());
        d.handle_mouse(test_packet(0, 0, false));
        d.settings_action(SettingsAction::ResetProfile);
        assert_eq!(d.wallpaper, Wallpaper::Ocean);
        let load = personalization::profile_button(
            rect, personalization::ProfileButton::Load).unwrap();
        d.pointer.x = load.x + 8;
        d.pointer.y = load.y + 8;
        assert!(d.handle_mouse(test_packet(0, 0, true)));
        assert_eq!(d.profile_status, ProfileStatus::Loaded);
        assert_eq!(d.wallpaper, profile.wallpaper);
        assert_eq!(d.accent, profile.accent);
        assert_eq!(d.dark, profile.dark);
    }

    #[test]
    fn invalid_and_missing_profile_cannot_change_desktop_preferences() {
        let mut d = Desktop::new();
        d.settings_action(SettingsAction::SetWallpaper(Wallpaper::Sunset));
        let original = (d.wallpaper, d.accent, d.dark);
        d.settings_action(SettingsAction::LoadProfile);
        assert_eq!(d.profile_status, ProfileStatus::NotFound);
        assert_eq!((d.wallpaper,d.accent,d.dark), original);
        d.ramfs.save(PROFILE_FILE, b"unsafe-profile").unwrap();
        d.settings_action(SettingsAction::LoadProfile);
        assert_eq!(d.profile_status, ProfileStatus::InvalidFile);
        assert_eq!((d.wallpaper,d.accent,d.dark), original);
    }

    #[test]
    fn full_ramfs_save_failure_does_not_corrupt_existing_files() {
        let mut d = Desktop::new();
        for name in [
            "A.TXT","B.TXT","C.TXT","D.TXT",
            "E.TXT","F.TXT","G.TXT","H.TXT",
        ] {
            d.ramfs.save(name,b"hello").unwrap();
        }
        let before = d.ramfs;
        d.settings_action(SettingsAction::SaveProfile);
        assert_eq!(d.profile_status, ProfileStatus::StorageFull);
        assert_eq!(d.ramfs, before);
    }

    #[test]
    fn custom_wallpaper_requires_validated_boot_asset() {
        let mut d = Desktop::new();
        d.settings_action(SettingsAction::SetWallpaper(Wallpaper::Custom));
        assert_eq!(d.wallpaper, Wallpaper::Ocean);
        assert_eq!(d.profile_status, ProfileStatus::ImageUnavailable);
        d.custom_wallpaper_available = true;
        d.settings_action(SettingsAction::SetWallpaper(Wallpaper::Custom));
        assert_eq!(d.wallpaper, Wallpaper::Custom);
        d.settings_action(SettingsAction::SaveProfile);
        assert_eq!(d.profile_status, ProfileStatus::Saved);
        let saved = d.ramfs;
        let mut no_asset = Desktop::new();
        no_asset.ramfs = saved;
        no_asset.settings_action(SettingsAction::LoadProfile);
        assert_eq!(no_asset.profile_status, ProfileStatus::ImageUnavailable);
        assert_eq!(no_asset.wallpaper, Wallpaper::Ocean);
        d.settings_action(SettingsAction::ResetProfile);
        d.settings_action(SettingsAction::LoadProfile);
        assert_eq!(d.wallpaper, Wallpaper::Custom);
        assert_eq!(d.profile_status, ProfileStatus::Loaded);
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
