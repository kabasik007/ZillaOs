//! Boot-time ZillaOS welcome and first-run configuration wizard.
//! All settings are volatile. NO partitioning, disk write, account creation,
//! RTC adjustment or OS installation is performed in this milestone.

use crate::desktop::{Key, Layout};
use crate::mouse::MousePacket;
use crate::storage_pci::StorageInventory;
use crate::window_manager::Move;

pub const DEVICE_NAME_MAX: usize = 24;
pub const STEP_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Language,
    Region,
    TimeZone,
    Device,
    Appearance,
    Storage,
    Review,
    Finished,
}

impl Step {
    pub const fn index(self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Language => 1,
            Self::Region => 2,
            Self::TimeZone => 3,
            Self::Device => 4,
            Self::Appearance => 5,
            Self::Storage => 6,
            Self::Review => 7,
            Self::Finished => 8,
        }
    }

    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Welcome,
            1 => Self::Language,
            2 => Self::Region,
            3 => Self::TimeZone,
            4 => Self::Device,
            5 => Self::Appearance,
            6 => Self::Storage,
            7 => Self::Review,
            _ => Self::Finished,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Ukrainian,
    English,
}

impl Language {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ukrainian => "uk",
            Self::English => "en",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Ukraine,
    Poland,
    UnitedStates,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeZone {
    Kyiv,
    Warsaw,
    NewYork,
    Utc,
}

impl TimeZone {
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Kyiv => "Europe/Kyiv",
            Self::Warsaw => "Europe/Warsaw",
            Self::NewYork => "America/New_York",
            Self::Utc => "Etc/UTC",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceName {
    bytes: [u8; DEVICE_NAME_MAX],
    len: u8,
}

impl DeviceName {
    pub const fn new() -> Self {
        let mut bytes = [0; DEVICE_NAME_MAX];
        // Default is ASCII, DNS-label-compatible and visible without a font.
        bytes[0] = b'z';
        bytes[1] = b'i';
        bytes[2] = b'l';
        bytes[3] = b'l';
        bytes[4] = b'a';
        bytes[5] = b'-';
        bytes[6] = b'p';
        bytes[7] = b'c';
        Self { bytes, len: 8 }
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: all mutations accept ASCII letters, numbers and hyphens.
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len as usize]) }
    }

    pub fn clear(&mut self) {
        self.bytes = [0; DEVICE_NAME_MAX];
        self.len = 0;
    }

    pub fn push(&mut self, ch: u8) -> bool {
        if !ch.is_ascii_alphanumeric() && ch != b'-' {
            return false;
        }
        if self.len as usize >= DEVICE_NAME_MAX {
            return false;
        }
        // Prevent invalid leading hyphen, preserving DNS label semantics.
        if self.len == 0 && ch == b'-' {
            return false;
        }
        self.bytes[self.len as usize] = ch.to_ascii_lowercase();
        self.len += 1;
        true
    }

    pub fn pop(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        self.len -= 1;
        self.bytes[self.len as usize] = 0;
        true
    }

    pub fn is_valid(&self) -> bool {
        self.len >= 3
            && self.bytes[0].is_ascii_alphanumeric()
            && self.bytes[self.len as usize - 1].is_ascii_alphanumeric()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupPointer {
    pub x: usize,
    pub y: usize,
    pub visible: bool,
    pub left: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wizard {
    pub step: Step,
    pub language: Language,
    pub region: Region,
    pub timezone: TimeZone,
    pub hostname: DeviceName,
    pub hostname_pristine: bool,
    pub storage: StorageInventory,
    pub dark: bool,
    pub selection: usize,
    pub pointer: SetupPointer,
    pub changes: u32,
    layout: Layout,
}

impl Wizard {
    pub fn new(layout: Layout) -> Self {
        Self {
            step: Step::Welcome,
            language: Language::Ukrainian,
            region: Region::Ukraine,
            timezone: TimeZone::Kyiv,
            hostname: DeviceName::new(),
            hostname_pristine: true,
            storage: StorageInventory::new(),
            dark: true,
            selection: 0,
            pointer: SetupPointer {
                x: layout.width / 2,
                y: layout.height / 2,
                visible: false,
                left: false,
            },
            changes: 0,
            layout,
        }
    }

    /// Hardware inventory may contain controllers, but NEVER implies a
    /// verified selectable block device or a writable installation target.
    pub fn set_storage_inventory(&mut self, inventory: StorageInventory) {
        self.storage = inventory;
    }

    pub const fn finished(&self) -> bool {
        matches!(self.step, Step::Finished)
    }

    pub const fn option_count(&self) -> usize {
        match self.step {
            Step::Language | Step::Appearance => 2,
            Step::Region => 3,
            Step::TimeZone => 4,
            _ => 0,
        }
    }

    pub const fn is_ukrainian(&self) -> bool {
        matches!(self.language, Language::Ukrainian)
    }

    pub fn localized(&self, uk: &'static str, en: &'static str) -> &'static str {
        if self.is_ukrainian() { uk } else { en }
    }

    /// Layout is shared with the renderer and click hit testing. On small
    /// screens the card fills nearly all usable space and has no sidebar.
    pub fn geometry(&self) -> WizardGeometry {
        WizardGeometry::new(self.layout)
    }

    pub fn enable_pointer(&mut self) {
        self.pointer.visible = true;
    }

    pub fn handle_key(&mut self, key: Key) -> bool {
        if self.finished() {
            return false;
        }
        let before = (self.step, self.language, self.region, self.timezone,
                      self.hostname, self.hostname_pristine, self.dark, self.selection);
        match key {
            Key::Confirm => self.next(),
            Key::Escape => self.previous(),
            Key::NextApp | Key::Move(Move::Right) | Key::Move(Move::Down) => self.shift_choice(true),
            Key::Move(Move::Left) | Key::Move(Move::Up) => self.shift_choice(false),
            Key::Backspace if self.step == Step::Device => {
                self.hostname_pristine = false;
                self.hostname.pop();
            }
            Key::Text(ch) if self.step == Step::Device => {
                if ch.is_ascii_alphanumeric() && self.hostname_pristine
                {
                    self.hostname.clear();
                    self.hostname_pristine = false;
                }
                self.hostname.push(ch);
            }
            _ => {}
        }
        self.apply_choice();
        let changed = before != (self.step, self.language, self.region, self.timezone,
                                 self.hostname, self.hostname_pristine, self.dark, self.selection);
        if changed {
            self.changes = self.changes.wrapping_add(1);
        }
        changed
    }

    fn shift_choice(&mut self, forward: bool) {
        let count = self.option_count();
        if count == 0 {
            return;
        }
        self.selection = if forward {
            (self.selection + 1) % count
        } else {
            (self.selection + count - 1) % count
        };
    }

    fn apply_choice(&mut self) {
        match self.step {
            Step::Language => self.language = if self.selection == 0 {
                Language::Ukrainian
            } else {
                Language::English
            },
            Step::Region => self.region = match self.selection {
                1 => Region::Poland,
                2 => Region::UnitedStates,
                _ => Region::Ukraine,
            },
            Step::TimeZone => self.timezone = match self.selection {
                1 => TimeZone::Warsaw,
                2 => TimeZone::NewYork,
                3 => TimeZone::Utc,
                _ => TimeZone::Kyiv,
            },
            Step::Appearance => self.dark = self.selection == 0,
            _ => {}
        }
    }

    fn selection_for_step(&self) -> usize {
        match self.step {
            Step::Language => if self.is_ukrainian() { 0 } else { 1 },
            Step::Region => match self.region {
                Region::Ukraine => 0,
                Region::Poland => 1,
                Region::UnitedStates => 2,
            },
            Step::TimeZone => match self.timezone {
                TimeZone::Kyiv => 0,
                TimeZone::Warsaw => 1,
                TimeZone::NewYork => 2,
                TimeZone::Utc => 3,
            },
            Step::Appearance => if self.dark { 0 } else { 1 },
            _ => 0,
        }
    }

    pub fn next(&mut self) {
        if self.step == Step::Device && !self.hostname.is_valid() {
            return;
        }
        if !self.finished() {
            self.step = Step::from_index(self.step.index() + 1);
            self.selection = self.selection_for_step();
        }
    }

    pub fn previous(&mut self) {
        if self.step.index() > 0 && !self.finished() {
            self.step = Step::from_index(self.step.index() - 1);
            self.selection = self.selection_for_step();
        }
    }

    pub fn handle_mouse(&mut self, packet: MousePacket) -> bool {
        if !self.pointer.visible || self.finished() {
            return false;
        }
        let old = *self;
        self.pointer.x = self.pointer.x
            .saturating_add_signed(packet.dx as isize)
            .min(self.layout.width.saturating_sub(1));
        self.pointer.y = self.pointer.y
            .saturating_add_signed(packet.dy as isize)
            .min(self.layout.height.saturating_sub(1));
        if packet.left && !self.pointer.left {
            let geometry = self.geometry();
            let x = self.pointer.x;
            let y = self.pointer.y;
            if geometry.next_contains(x, y) {
                self.next();
            } else if geometry.back_contains(x, y) {
                self.previous();
            } else if let Some(row) = geometry.option_at(x, y, self.option_count()) {
                self.selection = row;
                self.apply_choice();
            }
        }
        self.pointer.left = packet.left;
        let changed = *self != old;
        if changed {
            self.changes = self.changes.wrapping_add(1);
        }
        changed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WizardGeometry {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub sidebar: usize,
    pub body_x: usize,
    pub body_width: usize,
    pub option_y: usize,
    pub option_gap: usize,
    pub option_height: usize,
    pub footer_y: usize,
}

impl WizardGeometry {
    pub fn new(layout: Layout) -> Self {
        let width = layout.width.saturating_sub(24).min(970);
        let height = layout.height.saturating_sub(24).min(630);
        let x = layout.width.saturating_sub(width) / 2;
        let y = layout.height.saturating_sub(height) / 2;
        let sidebar = if width >= 780 { 240 } else { 0 };
        let body_x = x + sidebar + if width >= 500 { 40 } else { 16 };
        let body_width = width.saturating_sub(sidebar + if width >= 500 { 80 } else { 32 });
        let option_y = y + if height >= 470 { 188 } else { 108 };
        let option_gap = if height >= 470 { 66 } else { 37 };
        let option_height = option_gap.saturating_sub(6);
        let footer_y = y + height.saturating_sub(72);
        Self {
            x, y, width, height, sidebar, body_x, body_width,
            option_y, option_gap, option_height, footer_y,
        }
    }

    pub fn option_at(self, x: usize, y: usize, count: usize) -> Option<usize> {
        if x < self.body_x || x >= self.body_x.saturating_add(self.body_width) {
            return None;
        }
        (0..count).find(|index| {
            let top = self.option_y + index * self.option_gap;
            y >= top && y < top + self.option_height && top + self.option_height < self.footer_y
        })
    }

    pub fn next_contains(self, x: usize, y: usize) -> bool {
        let start = self.body_x + self.body_width.saturating_sub(162);
        x >= start && x < start + 162
            && y >= self.footer_y && y < self.footer_y + 44
    }

    pub fn back_contains(self, x: usize, y: usize) -> bool {
        x >= self.body_x && x < self.body_x + 112
            && y >= self.footer_y && y < self.footer_y + 44
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wizard() -> Wizard {
        Wizard::new(Layout::for_size(1024, 768).unwrap())
    }

    #[test]
    fn first_launch_uses_ukraine_kyiv_and_dark_defaults() {
        let s = wizard();
        assert_eq!(s.step, Step::Welcome);
        assert_eq!(s.language.code(), "uk");
        assert_eq!(s.timezone.identifier(), "Europe/Kyiv");
        assert_eq!(s.hostname.as_str(), "zilla-pc");
        assert!(s.dark);
        assert_eq!(s.storage.selectable_disks(), 0);
    }

    #[test]
    fn storage_step_never_makes_a_controller_selectable_as_disk() {
        use crate::storage_pci::{PciLocation, StorageController, StorageKind};
        let mut s = wizard();
        let mut inventory = StorageInventory::new();
        inventory.push(StorageController {
            location: PciLocation { bus: 0, device: 3, function: 0 },
            vendor: 0x15ad, device_id: 0x07e0,
            subclass: 6, programming_interface: 1,
            kind: StorageKind::SataAhci,
        });
        s.set_storage_inventory(inventory);
        for _ in 0..6 { s.handle_key(Key::Confirm); }
        assert_eq!(s.step, Step::Storage);
        assert_eq!(s.storage.count, 1);
        assert_eq!(s.storage.selectable_disks(), 0);
        assert_eq!(s.option_count(), 0);
        s.handle_key(Key::Confirm);
        assert_eq!(s.step, Step::Review);
    }

    #[test]
    fn full_keyboard_flow_enters_desktop_after_review() {
        let mut s = wizard();
        for _ in 0..STEP_COUNT {
            s.handle_key(Key::Confirm);
        }
        assert!(s.finished());
        assert!(!s.handle_key(Key::Confirm));
    }

    #[test]
    fn language_region_timezone_and_theme_are_independent() {
        let mut s = wizard();
        s.handle_key(Key::Confirm);
        s.handle_key(Key::Move(Move::Right));
        assert_eq!(s.language, Language::English);
        s.handle_key(Key::Confirm);
        s.handle_key(Key::Move(Move::Right));
        assert_eq!(s.region, Region::Poland);
        s.handle_key(Key::Confirm);
        s.handle_key(Key::Move(Move::Right));
        assert_eq!(s.timezone, TimeZone::Warsaw);
        s.handle_key(Key::Confirm);
        s.handle_key(Key::Confirm);
        s.handle_key(Key::Move(Move::Right));
        assert!(!s.dark);
    }

    #[test]
    fn hostname_is_bounded_and_rejects_invalid_labels() {
        let mut s = wizard();
        for _ in 0..4 { s.handle_key(Key::Confirm); }
        assert_eq!(s.step, Step::Device);
        for _ in 0..8 { s.handle_key(Key::Backspace); }
        assert!(!s.hostname.is_valid());
        assert!(!s.handle_key(Key::Confirm));
        assert_eq!(s.step, Step::Device);
        assert!(!s.handle_key(Key::Text(b'-')));
        for c in b"my-laptop" { s.handle_key(Key::Text(*c)); }
        assert_eq!(s.hostname.as_str(), "my-laptop");
        assert!(s.hostname.is_valid());
        s.handle_key(Key::Confirm);
        assert_eq!(s.step, Step::Appearance);
        let mut name = DeviceName::new();
        name.clear();
        for _ in 0..30 { name.push(b'x'); }
        assert_eq!(name.as_str().len(), DEVICE_NAME_MAX);
    }

    #[test]
    fn typing_custom_name_replaces_initial_suggestion() {
        let mut s = wizard();
        for _ in 0..4 { s.handle_key(Key::Confirm); }
        assert!(s.hostname_pristine);
        for letter in b"workstation-1" {
            s.handle_key(Key::Text(*letter));
        }
        assert_eq!(s.hostname.as_str(), "workstation-1");
        assert!(!s.hostname_pristine);
        assert!(s.hostname.is_valid());
    }

    #[test]
    fn previous_preserves_chosen_settings() {
        let mut s = wizard();
        s.handle_key(Key::Confirm);
        s.handle_key(Key::Move(Move::Right));
        s.handle_key(Key::Confirm);
        s.handle_key(Key::Escape);
        assert_eq!(s.step, Step::Language);
        assert_eq!(s.language, Language::English);
        assert_eq!(s.selection, 1);
    }

    #[test]
    fn layout_hit_testing_matches_options_and_footer() {
        for resolution in [(320, 240), (800, 600), (1024, 768), (1280, 800)] {
            let layout = Layout::for_size(resolution.0, resolution.1).unwrap();
            let g = WizardGeometry::new(layout);
            assert!(g.x + g.width <= layout.width);
            assert!(g.y + g.height <= layout.height);
            assert!(g.next_contains(g.body_x + g.body_width - 10, g.footer_y + 10));
            assert_eq!(g.option_at(g.body_x + 8, g.option_y + 4, 2), Some(0));
        }
    }

    #[test]
    fn mouse_changes_locale_and_next_page() {
        let mut s = wizard();
        s.enable_pointer();
        let g = s.geometry();
        s.pointer.x = g.body_x + g.body_width - 18;
        s.pointer.y = g.footer_y + 18;
        s.handle_mouse(MousePacket { dx: 0, dy: 0, left: true, right: false, middle: false });
        assert_eq!(s.step, Step::Language);
        s.handle_mouse(MousePacket { dx: 0, dy: 0, left: false, right: false, middle: false });
        s.pointer.x = g.body_x + 20;
        s.pointer.y = g.option_y + g.option_gap + 10;
        s.handle_mouse(MousePacket { dx: 0, dy: 0, left: true, right: false, middle: false });
        assert_eq!(s.language, Language::English);
    }
}
