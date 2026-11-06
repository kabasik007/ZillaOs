//! Shared geometry for the honest Zilla Task Manager surface list.
//! These are embedded windows, not independent user processes or threads.
use crate::apps::App;
use crate::window_manager::Rect;

pub const ROW_START: usize = 94;
pub const ROW_HEIGHT: usize = 37;
pub const ROW_GAP: usize = 41;

pub fn row_at(rect: Rect, x: usize, y: usize) -> Option<App> {
    let bar = if rect.height < 170 { 30 } else { 45 };
    let body = rect.y.saturating_add(bar + 8);
    let side = (rect.width / 4).clamp(70,170);
    let content_x = rect.x.saturating_add(side + 27);
    if x < content_x || x >= rect.x.saturating_add(rect.width.saturating_sub(12)) {
        return None;
    }
    let first = body.saturating_add(ROW_START);
    if y < first { return None; }
    let offset = y - first;
    if offset % ROW_GAP >= ROW_HEIGHT { return None; }
    let index = offset / ROW_GAP;
    // Prevent hit-testing beyond the actual rendered scroll-less viewport.
    if first + index * ROW_GAP + ROW_HEIGHT
        >= body + rect.height.saturating_sub(bar + 16).saturating_sub(8) {
        return None;
    }
    App::ALL.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rows_select_only_visible_embedded_surfaces() {
        let r=Rect { x:120,y:75,width:780,height:480 };
        let content_x=r.x+(r.width/4).clamp(70,170)+27;
        let start=r.y+45+8+ROW_START;
        for (i,app) in App::ALL.iter().enumerate() {
            assert_eq!(row_at(r,content_x+12,start+i*ROW_GAP+10),Some(*app));
            assert_eq!(row_at(r,content_x+12,start+i*ROW_GAP+ROW_HEIGHT+1),None);
        }
        assert_eq!(row_at(r,content_x-20,start+2),None);
        assert_eq!(row_at(r,content_x+12,start+App::COUNT*ROW_GAP+10),None);
    }
}
