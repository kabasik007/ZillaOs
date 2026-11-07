//! Shared mouse hit zones for the built-in text editor.
//! Graphics and interaction use identical coordinates.
use crate::window_manager::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    New,
    Save,
    Documents,
}

pub fn editor_action(rect: Rect, x: usize, y: usize) -> Option<EditorAction> {
    let bar = if rect.height < 170 { 30 } else { 45 };
    let body = rect.y.saturating_add(bar + 8);
    let side = (rect.width / 4).clamp(70, 170);
    let cx = rect.x.saturating_add(side + 27);
    if y < body + 37 || y >= body + 72 || x < cx {
        return None;
    }
    match x - cx {
        0..=79 => Some(EditorAction::New),
        88..=167 => Some(EditorAction::Save),
        176..=275 => Some(EditorAction::Documents),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn button_zones_match_renderer_and_cannot_activate_outside() {
        let rect = Rect {
            x: 120,
            y: 80,
            width: 650,
            height: 420,
        };
        let body = 80 + 45 + 8;
        let cx = 120 + (650 / 4).clamp(70, 170) + 27;
        assert_eq!(
            editor_action(rect, cx + 10, body + 50),
            Some(EditorAction::New)
        );
        assert_eq!(
            editor_action(rect, cx + 90, body + 50),
            Some(EditorAction::Save)
        );
        assert_eq!(
            editor_action(rect, cx + 177, body + 50),
            Some(EditorAction::Documents)
        );
        assert_eq!(editor_action(rect, cx - 3, body + 50), None);
        assert_eq!(editor_action(rect, cx + 20, body + 82), None);
    }
}
