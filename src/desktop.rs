//! Zilla Desktop: keyboard-driven, allocation-free desktop state machine.
//! This is a deliberately small GUI experiment, not a process manager.

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
    Escape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desktop {
    pub active: Option<App>,
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
    pub const fn new() -> Self {
        Self {
            active: Some(App::Files),
            launcher: false,
            dark: true,
            changes: 0,
        }
    }

    /// Return true only if visible state changed and a redraw is warranted.
    pub fn handle(&mut self, key: Key) -> bool {
        let before = (self.active, self.launcher, self.dark);
        match key {
            Key::Launcher => self.launcher = !self.launcher,
            Key::Files => {
                self.active = Some(App::Files);
                self.launcher = false;
            }
            Key::Terminal => {
                self.active = Some(App::Terminal);
                self.launcher = false;
            }
            Key::Settings => {
                self.active = Some(App::Settings);
                self.launcher = false;
            }
            Key::ToggleTheme => self.dark = !self.dark,
            Key::NextApp => {
                self.launcher = false;
                self.active = Some(match self.active {
                    None | Some(App::Settings) => App::Files,
                    Some(App::Files) => App::Terminal,
                    Some(App::Terminal) => App::Settings,
                });
            }
            Key::Escape => {
                if self.launcher {
                    self.launcher = false;
                } else {
                    self.active = None;
                }
            }
        }
        let changed = before != (self.active, self.launcher, self.dark);
        if changed {
            self.changes = self.changes.wrapping_add(1);
        }
        changed
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_files_window() {
        let desktop = Desktop::new();
        assert_eq!(desktop.active, Some(App::Files));
        assert!(desktop.dark);
        assert!(!desktop.launcher);
    }

    #[test]
    fn keyboard_launches_windows_and_cycle() {
        let mut state = Desktop::new();
        assert!(state.handle(Key::Terminal));
        assert_eq!(state.active, Some(App::Terminal));
        assert!(state.handle(Key::NextApp));
        assert_eq!(state.active, Some(App::Settings));
        assert!(state.handle(Key::NextApp));
        assert_eq!(state.active, Some(App::Files));
        assert!(!state.handle(Key::Files));
    }

    #[test]
    fn launcher_and_escape_have_expected_behavior() {
        let mut state = Desktop::new();
        assert!(state.handle(Key::Launcher));
        assert!(state.handle(Key::Escape));
        assert_eq!(state.active, Some(App::Files));
        assert!(state.handle(Key::Escape));
        assert_eq!(state.active, None);
        assert!(state.handle(Key::Files));
    }

    #[test]
    fn light_mode_can_toggle_both_ways() {
        let mut state = Desktop::new();
        assert!(state.handle(Key::ToggleTheme));
        assert!(!state.dark);
        assert!(state.handle(Key::ToggleTheme));
        assert!(state.dark);
        assert_eq!(state.changes, 2);
    }

    #[test]
    fn layout_stays_inside_framebuffer_at_typical_resolutions() {
        for (w, h) in [(320, 240), (640, 480), (800, 600), (1024, 768), (1920, 1080)] {
            let layout = Layout::for_size(w, h).unwrap();
            assert!(layout.window_x + layout.window_width <= w);
            assert!(layout.window_y + layout.window_height + layout.dock_height <= h);
            assert!(layout.window_y >= layout.top_bar);
        }
        assert!(Layout::for_size(240, 150).is_none());
    }
}
