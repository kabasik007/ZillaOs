//! Shared click geometry for the installed-program catalogue.
use crate::apps::{App, BUILTIN_APPS};
use crate::window_manager::Rect;

pub fn app_row_at(rect: Rect, x: usize, y: usize) -> Option<App> {
    let bar = if rect.height < 170 { 30 } else { 45 };
    let body = rect.y.saturating_add(bar + 8);
    let side = (rect.width / 4).clamp(70, 170);
    let content_x = rect.x.saturating_add(side + 27);
    if x < content_x || x >= rect.x.saturating_add(rect.width.saturating_sub(12)) {
        return None;
    }
    let start = body.saturating_add(47);
    if y < start { return None; }
    let offset = y - start;
    if offset % 43 >= 36 { return None; }
    BUILTIN_APPS.get(offset / 43).map(|descriptor| descriptor.id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rows_match_installed_registry_and_ignore_outside_clicks() {
        let rect = Rect { x: 100, y: 80, width: 620, height: 440 };
        let x = rect.x + (rect.width / 4).clamp(70, 170) + 35;
        let y = rect.y + 45 + 8 + 47;
        for (i, descriptor) in BUILTIN_APPS.iter().enumerate() {
            assert_eq!(app_row_at(rect, x, y + i * 43 + 5), Some(descriptor.id));
        }
        assert_eq!(app_row_at(rect, x - 300, y + 5), None);
        assert_eq!(app_row_at(rect, x, y + BUILTIN_APPS.len() * 43 + 5), None);
    }
}
