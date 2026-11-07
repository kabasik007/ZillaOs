//! Small no_std desktop personalization state and shared click geometry.
//! No filesystem persistence; wallpaper and accent apply to the current VM
//! session only. Input and renderer use the same deterministic hit boxes.

use crate::desktop::Layout;
use crate::setup::Language;
use crate::window_manager::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wallpaper {
    Ocean,
    Sunset,
    Forest,
}
impl Wallpaper {
    pub const fn next(self) -> Self {
        match self {
            Self::Ocean => Self::Sunset,
            Self::Sunset => Self::Forest,
            Self::Forest => Self::Ocean,
        }
    }
    pub const fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (Self::Ocean, Language::Ukrainian) => "ОКЕАН",
            (Self::Ocean, Language::English) => "OCEAN",
            (Self::Ocean, Language::Russian) => "ОКЕАН",
            (Self::Sunset, Language::Ukrainian) => "ЗАХІД СОНЦЯ",
            (Self::Sunset, Language::English) => "SUNSET",
            (Self::Sunset, Language::Russian) => "ЗАКАТ",
            (Self::Forest, Language::Ukrainian) => "ЛІС",
            (Self::Forest, Language::English) => "FOREST",
            (Self::Forest, Language::Russian) => "ЛЕС",
        }
    }
    /// Predefined colors, not image files: no disk or asset loader needed.
    pub const fn palette(self, dark: bool) -> (u32, u32, u32) {
        match (self, dark) {
            (Self::Ocean, true) => (0x061222, 0x0d3751, 0x167d91),
            (Self::Ocean, false) => (0xaed9f0, 0xe2f6fc, 0x438fa7),
            (Self::Sunset, true) => (0x241535, 0x873f64, 0xe6a16e),
            (Self::Sunset, false) => (0xf8b18b, 0xf6d4ba, 0xc87d72),
            (Self::Forest, true) => (0x081c22, 0x1e5649, 0x327f62),
            (Self::Forest, false) => (0xbedbd0, 0xe2efda, 0x589578),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accent {
    Teal,
    Violet,
    Amber,
}
impl Accent {
    pub const fn next(self) -> Self {
        match self {
            Self::Teal => Self::Violet,
            Self::Violet => Self::Amber,
            Self::Amber => Self::Teal,
        }
    }
    pub const fn rgb(self) -> u32 {
        match self {
            Self::Teal => 0x19c7b0,
            Self::Violet => 0xa687ed,
            Self::Amber => 0xedac60,
        }
    }
    pub const fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (Self::Teal, Language::Ukrainian) => "БІРЮЗОВИЙ",
            (Self::Teal, Language::English) => "TEAL",
            (Self::Teal, Language::Russian) => "БИРЮЗОВЫЙ",
            (Self::Violet, Language::Ukrainian) => "ФІОЛЕТОВИЙ",
            (Self::Violet, Language::English) => "VIOLET",
            (Self::Violet, Language::Russian) => "ФИОЛЕТОВЫЙ",
            (Self::Amber, Language::Ukrainian) => "БУРШТИНОВИЙ",
            (Self::Amber, Language::English) => "AMBER",
            (Self::Amber, Language::Russian) => "ЯНТАРНЫЙ",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Overview,
    Personalization,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    ToggleTheme,
    CycleLanguage,
    OpenPersonalization,
    Back,
    CycleWallpaper,
    CycleAccent,
}

/// Renderer uses precisely these vertical intervals and the same body origin.
pub fn settings_hit(rect: Rect, page: SettingsPage, x: usize, y: usize) -> Option<SettingsAction> {
    let bar_h = if rect.height < 170 { 30 } else { 45 };
    let body = rect.y.saturating_add(bar_h + 8);
    let content_h = rect.height.saturating_sub(bar_h + 16);
    let side = (rect.width / 4).clamp(70, 170);
    let cx = rect.x.saturating_add(side + 27);
    if x < cx || x >= rect.x.saturating_add(rect.width).saturating_sub(17)
        || y < body || y >= body.saturating_add(content_h)
    {
        return None;
    }
    let offset = y - body;
    let rows: &[(usize, usize, SettingsAction)] = match page {
        SettingsPage::Overview => &[
            (42, 93, SettingsAction::ToggleTheme),
            (105, 157, SettingsAction::CycleLanguage),
            (175, 226, SettingsAction::OpenPersonalization),
        ],
        SettingsPage::Personalization => &[
            (8, 31, SettingsAction::Back),
            (56, 107, SettingsAction::CycleWallpaper),
            (119, 170, SettingsAction::CycleAccent),
            (182, 233, SettingsAction::ToggleTheme),
        ],
    };
    for (start, end, action) in rows {
        if offset >= *start && offset < *end && content_h >= *end + 2 {
            return Some(*action);
        }
    }
    None
}

/// A desktop-only popup. Two genuine actions: personalize, display settings.
pub const MENU_WIDTH: usize = 230;
pub const MENU_HEIGHT: usize = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Personalization,
    DisplaySettings,
}

impl Menu {
    pub fn at(layout: Layout, click_x: usize, click_y: usize) -> Self {
        let width = MENU_WIDTH.min(layout.width);
        let height = MENU_HEIGHT.min(layout.height);
        // Keep it clear of the top bar and taskbar when the screen allows.
        let bottom = layout.height.saturating_sub(layout.dock_height + 12);
        let max_y = bottom.saturating_sub(height).max(layout.top_bar);
        let x = click_x.min(layout.width.saturating_sub(width));
        let y = click_y.min(max_y).max(layout.top_bar.min(max_y));
        Self { x, y, width, height }
    }

    pub fn contains(self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.x.saturating_add(self.width)
            && y >= self.y && y < self.y.saturating_add(self.height)
    }

    pub fn hit(self, x: usize, y: usize) -> Option<MenuAction> {
        if !self.contains(x, y) { return None; }
        let relative = y - self.y;
        if (8..43).contains(&relative) {
            Some(MenuAction::Personalization)
        } else if (47..82).contains(&relative) {
            Some(MenuAction::DisplaySettings)
        } else {
            None
        }
    }
}

/// Integer-only RGB interpolation for a software-rendered gradient.
pub fn gradient(top: u32, bottom: u32, fraction: u32) -> u32 {
    let t = fraction.min(255);
    let channel = |shift: u32| {
        let a = (top >> shift) & 255;
        let b = (bottom >> shift) & 255;
        (a * (255 - t) + b * t) / 255
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::Layout;

    #[test]
    fn presets_cycle_without_allocation() {
        let mut w = Wallpaper::Ocean;
        for _ in 0..3 { w = w.next(); }
        assert_eq!(w, Wallpaper::Ocean);
        let mut accent = Accent::Teal;
        for _ in 0..3 { accent = accent.next(); }
        assert_eq!(accent, Accent::Teal);
        assert_ne!(Wallpaper::Forest.palette(true), Wallpaper::Forest.palette(false));
    }

    #[test]
    fn gradient_endpoints_exact_and_channels_bounded() {
        assert_eq!(gradient(0x102030, 0xe0a070, 0), 0x102030);
        assert_eq!(gradient(0x102030, 0xe0a070, 255), 0xe0a070);
        assert_eq!(gradient(0x102030, 0xe0a070, u32::MAX), 0xe0a070);
    }

    #[test]
    fn menu_clamps_within_desktop_and_hit_boxes_match() {
        for (w,h) in [(320,240),(800,600),(1024,768),(1920,1080)] {
            let layout = Layout::for_size(w,h).unwrap();
            let m = Menu::at(layout,usize::MAX,usize::MAX);
            assert!(m.x+m.width<=w && m.y+m.height<=h);
            assert_eq!(m.hit(m.x+5,m.y+10),Some(MenuAction::Personalization));
            assert_eq!(m.hit(m.x+5,m.y+55),Some(MenuAction::DisplaySettings));
            assert_eq!(m.hit(usize::MAX,usize::MAX),None);
        }
    }

    #[test]
    fn settings_overview_and_personalization_share_click_geometry() {
        let rect = Rect{x:100,y:100,width:600,height:400};
        let cx = rect.x + (rect.width/4).clamp(70,170)+40;
        let y = rect.y + 45 + 8;
        assert_eq!(settings_hit(rect,SettingsPage::Overview,cx,y+62),Some(SettingsAction::ToggleTheme));
        assert_eq!(settings_hit(rect,SettingsPage::Overview,cx,y+130),Some(SettingsAction::CycleLanguage));
        assert_eq!(settings_hit(rect,SettingsPage::Overview,cx,y+195),Some(SettingsAction::OpenPersonalization));
        assert_eq!(settings_hit(rect,SettingsPage::Personalization,cx,y+60),Some(SettingsAction::CycleWallpaper));
        assert_eq!(settings_hit(rect,SettingsPage::Personalization,cx,y+130),Some(SettingsAction::CycleAccent));
        assert_eq!(settings_hit(rect,SettingsPage::Personalization,cx,y+190),Some(SettingsAction::ToggleTheme));
        assert_eq!(settings_hit(rect,SettingsPage::Personalization,cx,y+16),Some(SettingsAction::Back));
        assert_eq!(settings_hit(rect,SettingsPage::Overview,100,y+62),None);
    }

    #[test]
    fn labels_exist_in_three_languages() {
        for lang in [Language::Ukrainian,Language::English,Language::Russian] {
            for w in [Wallpaper::Ocean,Wallpaper::Sunset,Wallpaper::Forest] { assert!(!w.name(lang).is_empty()); }
            for a in [Accent::Teal,Accent::Violet,Accent::Amber] { assert!(!a.name(lang).is_empty()); }
        }
    }
}
