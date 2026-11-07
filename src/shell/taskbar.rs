//! Shared, testable no_std click geometry for the Zilla hybrid taskbar.
//! Mac-style centered dock remains separate. The two left buttons have real
//! actions; the right status area is deliberately read-only.
use crate::desktop::Layout;
use crate::setup::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start,
    Tasks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}
impl ButtonRect {
    pub fn contains(self, x: usize, y: usize) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width)
            && y >= self.y
            && y < self.y.saturating_add(self.height)
    }
}
pub const START_X: usize = 14;
pub const TASKS_X: usize = 96;

pub fn button(layout: Layout, action: Action) -> Option<ButtonRect> {
    // The centered Dock occupies the left side at narrower widths.
    if action == Action::Tasks && layout.width < 960 {
        return None;
    }
    let x = if action == Action::Start {
        START_X
    } else {
        TASKS_X
    };
    let width = if action == Action::Start { 74 } else { 77 };
    let y = layout.height.saturating_sub(layout.dock_height + 1);
    let height = layout.dock_height.saturating_sub(20).min(48);
    if x + width > layout.width {
        return None;
    }
    Some(ButtonRect {
        x,
        y,
        width,
        height,
    })
}

pub fn hit(layout: Layout, x: usize, y: usize) -> Option<Action> {
    for action in [Action::Start, Action::Tasks] {
        if button(layout, action).is_some_and(|rect| rect.contains(x, y)) {
            return Some(action);
        }
    }
    None
}

// The text in the top bar is action-oriented rather than a collection of
// unexplained menu names. Renderer and hit tester use the exact same labels.
pub const MENU_COUNT: usize = 5;
pub fn menu_label(language: Language, index: usize) -> &'static str {
    match (language, index) {
        (Language::Ukrainian, 0) => "ЗАСТОСУНКИ",
        (Language::Ukrainian, 1) => "ФАЙЛИ",
        (Language::Ukrainian, 2) => "ТЕМА",
        (Language::Ukrainian, 3) => "РОБОЧИЙ СТІЛ",
        (Language::Ukrainian, 4) => "ЗАДАЧІ",
        (Language::Russian, 0) => "ПРОГРАММЫ",
        (Language::Russian, 1) => "ФАЙЛЫ",
        (Language::Russian, 2) => "ТЕМА",
        (Language::Russian, 3) => "РАБОЧИЙ СТОЛ",
        (Language::Russian, 4) => "ЗАДАЧИ",
        (Language::English, 0) => "APPS",
        (Language::English, 1) => "FILES",
        (Language::English, 2) => "THEME",
        (Language::English, 3) => "DESKTOP",
        (Language::English, 4) => "TASKS",
        _ => "",
    }
}
pub fn menu_hit(layout: Layout, language: Language, x: usize, y: usize) -> Option<usize> {
    if y >= layout.top_bar || layout.width < 720 {
        return None;
    }
    let mut start = 170usize;
    for index in 0..MENU_COUNT {
        let end = start + menu_label(language, index).chars().count() * 6 + 16;
        if x >= start && x < end {
            return Some(index);
        }
        start = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buttons_are_real_and_do_not_overlap_center_dock() {
        let l = Layout::for_size(1024, 768).unwrap();
        let start = button(l, Action::Start).unwrap();
        let tasks = button(l, Action::Tasks).unwrap();
        assert_eq!(hit(l, start.x + 10, start.y + 10), Some(Action::Start));
        assert_eq!(hit(l, tasks.x + 10, tasks.y + 10), Some(Action::Tasks));
        assert_eq!(hit(l, 500, tasks.y + 10), None);
        assert!(button(Layout::for_size(800, 600).unwrap(), Action::Tasks).is_none());
        assert!(button(Layout::for_size(320, 240).unwrap(), Action::Start).is_some());
    }
    #[test]
    fn top_menu_labels_and_click_zones_match() {
        let l = Layout::for_size(1024, 768).unwrap();
        for language in [Language::Ukrainian, Language::English, Language::Russian] {
            let mut x = 170;
            for i in 0..MENU_COUNT {
                assert!(!menu_label(language, i).is_empty());
                assert_eq!(menu_hit(l, language, x + 2, 18), Some(i));
                x += menu_label(language, i).chars().count() * 6 + 16;
            }
            assert_eq!(menu_hit(l, language, x + 3, 18), None);
            assert_eq!(menu_hit(l, language, 170, 45), None);
        }
    }
}
