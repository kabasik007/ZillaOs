//! Pointer hit-testing for the built-in Files app.
//! Kept independent from the renderer so pixels and clickable rows agree.
use crate::window_manager::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesAction {
    Home,
    Up,
    OpenRow(usize),
}

pub fn files_action(rect: Rect, x: usize, y: usize) -> Option<FilesAction> {
    let bar_height = if rect.height < 170 { 30 } else { 45 };
    let body = rect.y.saturating_add(bar_height + 8);
    let side_width = (rect.width / 4).clamp(70, 170);
    let content_x = rect.x.saturating_add(side_width + 27);
    if x >= rect.x.saturating_add(12)
        && x < rect.x.saturating_add(10 + side_width)
        && y >= body.saturating_add(6)
        && y < body.saturating_add(38)
    {
        return Some(FilesAction::Home);
    }
    if x < content_x || x >= rect.x.saturating_add(rect.width.saturating_sub(12)) {
        return None;
    }
    if y >= body.saturating_add(7) && y < body.saturating_add(35) {
        return Some(FilesAction::Up);
    }
    let start = body.saturating_add(42);
    if y >= start {
        let offset = y - start;
        let row = offset / 47;
        if offset % 47 < 40 && row < 7 {
            return Some(FilesAction::OpenRow(row));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_actions_follow_layout_and_reject_outside_clicks() {
        let rect = Rect { x: 120, y: 80, width: 600, height: 420 };
        let body = 80 + 45 + 8;
        let cx = 120 + (600 / 4) + 40;
        assert_eq!(files_action(rect, 140, body + 10), Some(FilesAction::Home));
        assert_eq!(files_action(rect, cx, body + 15), Some(FilesAction::Up));
        assert_eq!(files_action(rect, cx, body + 42 + 47 * 2 + 12),
                   Some(FilesAction::OpenRow(2)));
        assert_eq!(files_action(rect, 80, body + 60), None);
        assert_eq!(files_action(rect, cx, body + 42 + 45), None);
    }
}
