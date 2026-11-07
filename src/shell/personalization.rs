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
    /// Optional validated BMP asset bundled in the boot ISO.
    Custom,
}
impl Wallpaper {
    pub const fn next(self) -> Self {
        match self {
            Self::Ocean => Self::Sunset,
            Self::Sunset => Self::Forest,
            Self::Forest => Self::Ocean,
            Self::Custom => Self::Ocean,
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
            (Self::Custom, Language::Ukrainian) => "МОЇ ШПАЛЕРИ",
            (Self::Custom, Language::English) => "MY WALLPAPER",
            (Self::Custom, Language::Russian) => "МОИ ОБОИ",
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
            // Safe procedural fallback if the module is absent or invalid.
            (Self::Custom, true) => (0x061222, 0x0d3751, 0x167d91),
            (Self::Custom, false) => (0xaed9f0, 0xe2f6fc, 0x438fa7),
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
    SetWallpaper(Wallpaper),
    SetAccent(Accent),
    SaveProfile,
    LoadProfile,
    ResetProfile,
}

/// Direct preset thumbnails and profile buttons share coordinates between
/// hit-testing and the software renderer. They appear only when the Settings
/// content is wide/tall enough; compact modes retain the existing cycle rows.
pub const WALLPAPER_OPTIONS: [Wallpaper; 4] =
    [Wallpaper::Ocean, Wallpaper::Sunset, Wallpaper::Forest, Wallpaper::Custom];
pub const ACCENT_OPTIONS: [Accent; 3] =
    [Accent::Teal, Accent::Violet, Accent::Amber];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GalleryRow { Wallpaper, Accent }

pub fn content_geometry(rect: Rect) -> (usize, usize, usize, usize) {
    let bar_h = if rect.height < 170 { 30 } else { 45 };
    let body = rect.y.saturating_add(bar_h + 8);
    let content_h = rect.height.saturating_sub(bar_h + 16);
    let side = (rect.width / 4).clamp(70, 170);
    let cx = rect.x.saturating_add(side + 27);
    let cw = rect.width.saturating_sub(side + 44);
    (cx, cw, body, content_h)
}

pub fn preset_tile(rect: Rect, row: GalleryRow, index: usize) -> Option<Rect> {
    if index >= match row { GalleryRow::Wallpaper => 4, GalleryRow::Accent => 3 } {
        return None;
    }
    let (cx, cw, body, h) = content_geometry(rect);
    let dy = match row {
        GalleryRow::Wallpaper => 64,
        GalleryRow::Accent => 127,
    };
    if cw < 420 || h < dy + 36 { return None; }
    Some(Rect {
        x: cx + cw - 214 + index * 52,
        y: body + dy,
        width: 45,
        height: 34,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileButton { Save, Load, Reset }

impl ProfileButton {
    pub const fn label(self, language: Language) -> &'static str {
        use Language::{Ukrainian, English, Russian};
        match (self, language) {
            (Self::Save, Ukrainian) => "ЗБЕРЕГТИ",
            (Self::Save, English) => "SAVE RAM",
            (Self::Save, Russian) => "СОХРАНИТЬ",
            (Self::Load, Ukrainian) => "ЗАВАНТАЖИТИ",
            (Self::Load, English) => "LOAD RAM",
            (Self::Load, Russian) => "ЗАГРУЗИТЬ",
            (Self::Reset, Ukrainian) => "СКИНУТИ",
            (Self::Reset, English) => "RESET",
            (Self::Reset, Russian) => "СБРОС",
        }
    }
}

pub fn profile_button(rect: Rect, button: ProfileButton) -> Option<Rect> {
    let (cx, cw, body, h) = content_geometry(rect);
    if cw < 330 || h < 339 { return None; }
    let index = match button {
        ProfileButton::Save => 0,
        ProfileButton::Load => 1,
        ProfileButton::Reset => 2,
    };
    let width = (cw - 20) / 3;
    Some(Rect {
        x: cx + index * (width + 10),
        y: body + 292,
        width,
        height: 38,
    })
}

pub fn rect_contains(rect: Rect, x: usize, y: usize) -> bool {
    x >= rect.x && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
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
    if page == SettingsPage::Personalization {
        for i in 0..3 {
            if preset_tile(rect, GalleryRow::Wallpaper, i)
                .is_some_and(|r| rect_contains(r, x, y))
            {
                return Some(SettingsAction::SetWallpaper(WALLPAPER_OPTIONS[i]));
            }
            if preset_tile(rect, GalleryRow::Accent, i)
                .is_some_and(|r| rect_contains(r, x, y))
            {
                return Some(SettingsAction::SetAccent(ACCENT_OPTIONS[i]));
            }
        }
        for (button, action) in [
            (ProfileButton::Save, SettingsAction::SaveProfile),
            (ProfileButton::Load, SettingsAction::LoadProfile),
            (ProfileButton::Reset, SettingsAction::ResetProfile),
        ] {
            if profile_button(rect, button)
                .is_some_and(|r| rect_contains(r, x, y))
            {
                return Some(action);
            }
        }
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
    fn every_gallery_swatch_hits_its_exact_preset() {
        let rect = Rect{x:100,y:80,width:750,height:460};
        for i in 0..4 {
            let wallpaper = preset_tile(rect, GalleryRow::Wallpaper, i).unwrap();
            assert_eq!(
                settings_hit(rect, SettingsPage::Personalization,
                             wallpaper.x+8, wallpaper.y+8),
                Some(SettingsAction::SetWallpaper(WALLPAPER_OPTIONS[i]))
            );
            let accent = preset_tile(rect, GalleryRow::Accent, i).unwrap();
            assert_eq!(
                settings_hit(rect, SettingsPage::Personalization,
                             accent.x+8, accent.y+8),
                Some(SettingsAction::SetAccent(ACCENT_OPTIONS[i]))
            );
        }
    }

    #[test]
    fn profile_buttons_respect_actual_window_size() {
        let big = Rect{x:100,y:80,width:750,height:460};
        for (button, action) in [
            (ProfileButton::Save, SettingsAction::SaveProfile),
            (ProfileButton::Load, SettingsAction::LoadProfile),
            (ProfileButton::Reset, SettingsAction::ResetProfile),
        ] {
            let r=profile_button(big,button).unwrap();
            assert_eq!(
                settings_hit(big,SettingsPage::Personalization,r.x+5,r.y+5),
                Some(action)
            );
        }
        let small = Rect{x:5,y:5,width:280,height:200};
        assert!(profile_button(small,ProfileButton::Save).is_none());
        assert!(preset_tile(small, GalleryRow::Wallpaper,0).is_none());
    }

    #[test]
    fn fourth_wallpaper_slot_never_becomes_fourth_accent_slot() {
        let r = Rect{x:100,y:80,width:750,height:460};
        assert!(preset_tile(r, GalleryRow::Wallpaper, 3).is_some());
        assert!(preset_tile(r, GalleryRow::Accent, 3).is_none());
    }

    #[test]
    fn optional_custom_slot_stays_inside_gallery() {
        let rect = Rect{x:100,y:80,width:750,height:460};
        let custom = preset_tile(rect, GalleryRow::Wallpaper, 3).unwrap();
        let (cx, cw, _, _) = content_geometry(rect);
        assert!(custom.x >= cx && custom.x + custom.width <= cx + cw);
        assert_eq!(settings_hit(rect, SettingsPage::Personalization,
                   custom.x + 5, custom.y + 5),
                   Some(SettingsAction::SetWallpaper(Wallpaper::Custom)));
        assert_eq!(Wallpaper::Custom.next(), Wallpaper::Ocean);
    }

    #[test]
    fn profile_buttons_are_localized() {
        for language in [Language::Ukrainian, Language::English, Language::Russian] {
            for button in [ProfileButton::Save, ProfileButton::Load, ProfileButton::Reset] {
                assert!(!button.label(language).is_empty());
            }
        }
    }

    #[test]
    fn labels_exist_in_three_languages() {
        for lang in [Language::Ukrainian,Language::English,Language::Russian] {
            for w in [Wallpaper::Ocean,Wallpaper::Sunset,Wallpaper::Forest] { assert!(!w.name(lang).is_empty()); }
            for a in [Accent::Teal,Accent::Violet,Accent::Amber] { assert!(!a.name(lang).is_empty()); }
        }
    }
}
