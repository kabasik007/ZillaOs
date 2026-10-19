//! Minimal deterministic floating-window manager for the Zilla Desktop VM.
//!
//! Pure no_std state: no heap, pointers, filesystem or privileged resources.
//! The three Window records stand for DEMO VIEWS, not user-space processes.

use crate::desktop::{App, Layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    Hidden,
    Normal,
    Minimized,
    Maximized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub app: App,
    pub rect: Rect,
    pub mode: WindowMode,
}

impl Window {
    pub const fn visible(self) -> bool {
        matches!(self.mode, WindowMode::Normal | WindowMode::Maximized)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowManager {
    windows: [Window; 3],
    // Back to front, with the highest indexed visible window focused.
    order: [App; 3],
    focus: Option<App>,
    viewport: Layout,
}

impl WindowManager {
    pub fn new(viewport: Layout) -> Self {
        // Keep room for a visible cascade even at 800x600; the original
        // layout window occupies almost the entire screen.
        let width = viewport.window_width.min((viewport.width * 78 / 100).max(240));
        let height = viewport.window_height.min((viewport.height * 64 / 100).max(100));
        let base = Self::clamp_rect(Rect {
            x: viewport.width.saturating_sub(width) / 2,
            y: viewport.top_bar.saturating_add(25),
            width,
            height,
        }, viewport);
        let offset = (viewport.width / 20).clamp(12, 40);
        let shifted = Self::clamp_rect(
            Rect {
                x: base.x.saturating_add(offset),
                y: base.y.saturating_add(offset),
                ..base
            },
            viewport,
        );
        let shifted_again = Self::clamp_rect(
            Rect {
                x: base.x.saturating_add(offset * 2),
                y: base.y.saturating_add(offset * 2),
                ..base
            },
            viewport,
        );
        Self {
            windows: [
                Window { app: App::Files, rect: base, mode: WindowMode::Normal },
                Window { app: App::Terminal, rect: shifted, mode: WindowMode::Hidden },
                Window { app: App::Settings, rect: shifted_again, mode: WindowMode::Hidden },
            ],
            order: [App::Settings, App::Terminal, App::Files],
            focus: Some(App::Files),
            viewport,
        }
    }

    const fn index(app: App) -> usize {
        match app {
            App::Files => 0,
            App::Terminal => 1,
            App::Settings => 2,
        }
    }

    pub const fn focused(&self) -> Option<App> {
        self.focus
    }

    pub fn window(&self, app: App) -> Window {
        self.windows[Self::index(app)]
    }

    pub const fn back_to_front(&self) -> [App; 3] {
        self.order
    }

    pub fn rect(&self, app: App) -> Rect {
        let window = self.window(app);
        if window.mode == WindowMode::Maximized {
            let top = self.viewport.top_bar.saturating_add(8);
            let bottom = self.bottom();
            Rect {
                x: 8,
                y: top,
                width: self.viewport.width.saturating_sub(16),
                height: bottom.saturating_sub(top),
            }
        } else {
            window.rect
        }
    }

    fn bottom(&self) -> usize {
        self.viewport.height.saturating_sub(self.viewport.dock_height + 18)
    }

    fn clamp_rect(mut rect: Rect, viewport: Layout) -> Rect {
        let top = viewport.top_bar.saturating_add(8);
        let bottom = viewport.height.saturating_sub(viewport.dock_height + 18);
        let max_height = bottom.saturating_sub(top);
        rect.width = rect.width.clamp(180.min(viewport.width), viewport.width.saturating_sub(16).max(1));
        rect.height = rect.height.clamp(80.min(max_height), max_height.max(1));
        rect.x = rect.x.clamp(8, viewport.width.saturating_sub(rect.width + 8).max(8));
        rect.y = rect.y.clamp(top, bottom.saturating_sub(rect.height).max(top));
        rect
    }

    fn frontmost_visible(&self) -> Option<App> {
        self.order.iter().rev().copied().find(|app| self.window(*app).visible())
    }

    fn bring_to_front(&mut self, app: App) {
        let index = self.order.iter().position(|candidate| *candidate == app);
        if let Some(index) = index {
            for n in index..2 {
                self.order[n] = self.order[n + 1];
            }
            self.order[2] = app;
        }
        self.focus = Some(app);
    }

    pub fn open(&mut self, app: App) {
        let window = &mut self.windows[Self::index(app)];
        if matches!(window.mode, WindowMode::Hidden | WindowMode::Minimized) {
            window.mode = WindowMode::Normal;
        }
        self.bring_to_front(app);
    }

    pub fn cycle(&mut self) {
        let available: [App; 3] = self.order;
        let start = self.focus.and_then(|focus| available.iter().position(|app| *app == focus));
        // Choose the next visible window in stable cyclic Z order.
        for distance in 1..=3 {
            let candidate = available[(start.unwrap_or(2) + distance) % 3];
            if self.window(candidate).visible() {
                self.bring_to_front(candidate);
                return;
            }
        }
    }

    pub fn close_focused(&mut self) {
        if let Some(app) = self.focus {
            self.windows[Self::index(app)].mode = WindowMode::Hidden;
            self.focus = self.frontmost_visible();
        }
    }

    pub fn minimize_focused(&mut self) {
        if let Some(app) = self.focus {
            self.windows[Self::index(app)].mode = WindowMode::Minimized;
            self.focus = self.frontmost_visible();
        }
    }

    pub fn toggle_maximize(&mut self) {
        if let Some(app) = self.focus {
            let window = &mut self.windows[Self::index(app)];
            window.mode = match window.mode {
                WindowMode::Normal => WindowMode::Maximized,
                WindowMode::Maximized => WindowMode::Normal,
                other => other,
            };
        }
    }

    pub fn translate_focused(&mut self, direction: Move) {
        let Some(app) = self.focus else { return };
        let i = Self::index(app);
        if self.windows[i].mode != WindowMode::Normal { return; }
        let mut rect = self.windows[i].rect;
        let step = 24;
        match direction {
            Move::Left => rect.x = rect.x.saturating_sub(step),
            Move::Right => rect.x = rect.x.saturating_add(step),
            Move::Up => rect.y = rect.y.saturating_sub(step),
            Move::Down => rect.y = rect.y.saturating_add(step),
        }
        self.windows[i].rect = Self::clamp_rect(rect, self.viewport);
    }

    pub fn resize_focused(&mut self, direction: Move) {
        let Some(app) = self.focus else { return };
        let i = Self::index(app);
        if self.windows[i].mode != WindowMode::Normal { return; }
        let mut rect = self.windows[i].rect;
        let step = 24;
        match direction {
            Move::Left => rect.width = rect.width.saturating_sub(step),
            Move::Right => rect.width = rect.width.saturating_add(step),
            Move::Up => rect.height = rect.height.saturating_sub(step),
            Move::Down => rect.height = rect.height.saturating_add(step),
        }
        self.windows[i].rect = Self::clamp_rect(rect, self.viewport);
    }

    pub fn visible_count(&self) -> usize {
        self.windows.iter().filter(|win| win.visible()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> WindowManager {
        WindowManager::new(Layout::for_size(1024, 768).unwrap())
    }

    #[test]
    fn opens_distinct_overlap_windows_and_preserves_focus() {
        let mut wm = manager();
        assert_eq!(wm.visible_count(), 1);
        wm.open(App::Terminal);
        wm.open(App::Settings);
        assert_eq!(wm.visible_count(), 3);
        assert_eq!(wm.focused(), Some(App::Settings));
        assert_eq!(wm.back_to_front()[2], App::Settings);
        wm.open(App::Files);
        assert_eq!(wm.back_to_front()[2], App::Files);
    }

    #[test]
    fn minimize_and_restore_via_app_launcher() {
        let mut wm = manager();
        wm.open(App::Terminal);
        wm.minimize_focused();
        assert_eq!(wm.window(App::Terminal).mode, WindowMode::Minimized);
        assert_eq!(wm.focused(), Some(App::Files));
        wm.open(App::Terminal);
        assert_eq!(wm.window(App::Terminal).mode, WindowMode::Normal);
    }

    #[test]
    fn maximize_restores_previous_window_geometry() {
        let mut wm = manager();
        let saved = wm.rect(App::Files);
        wm.toggle_maximize();
        assert_eq!(wm.window(App::Files).mode, WindowMode::Maximized);
        assert!(wm.rect(App::Files).width > saved.width);
        wm.toggle_maximize();
        assert_eq!(wm.rect(App::Files), saved);
    }

    #[test]
    fn move_and_resize_respect_display_boundaries() {
        let mut wm = manager();
        for _ in 0..100 {
            wm.translate_focused(Move::Right);
            wm.translate_focused(Move::Down);
            wm.resize_focused(Move::Right);
            wm.resize_focused(Move::Down);
        }
        let rect = wm.rect(App::Files);
        assert!(rect.x + rect.width <= 1024);
        assert!(rect.y + rect.height <= 768 - 72 - 18);
        for _ in 0..100 {
            wm.resize_focused(Move::Left);
            wm.resize_focused(Move::Up);
        }
        assert!(wm.rect(App::Files).width >= 180);
        assert!(wm.rect(App::Files).height >= 80);
    }

    #[test]
    fn close_focused_keeps_other_window_alive() {
        let mut wm = manager();
        wm.open(App::Settings);
        wm.close_focused();
        assert_eq!(wm.window(App::Settings).mode, WindowMode::Hidden);
        assert_eq!(wm.focused(), Some(App::Files));
    }

    #[test]
    fn cycle_only_uses_visible_windows() {
        let mut wm = manager();
        wm.cycle();
        assert_eq!(wm.focused(), Some(App::Files));
        wm.open(App::Terminal);
        wm.cycle();
        assert_eq!(wm.focused(), Some(App::Files));
        wm.cycle();
        assert_eq!(wm.focused(), Some(App::Terminal));
    }

    #[test]
    fn compact_screen_never_exceeds_safe_layout() {
        let layout = Layout::for_size(320, 240).unwrap();
        let mut wm = WindowManager::new(layout);
        wm.open(App::Settings);
        wm.toggle_maximize();
        let rect = wm.rect(App::Settings);
        assert!(rect.x + rect.width <= 320);
        assert!(rect.y + rect.height <= 240);
    }
}
