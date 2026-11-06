//! Non-overlapping top-bar status layout for the ZillaOS bitmap font.
//! All coordinates are right-anchored; optional items disappear on narrow
//! screens rather than being painted over the hostname or menu.
use crate::desktop::Layout;
use crate::setup::Language;
use super::taskbar;

const FONT_WIDTH: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot { pub x: usize, pub width: usize }

impl Slot {
    const fn ends_at(self) -> usize { self.x + self.width }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusLayout {
    pub microphone: Option<Slot>,
    pub hostname: Option<Slot>,
    pub date: Option<Slot>,
    pub time: Slot,
    pub rtc: Slot,
}

pub fn menu_end(language: Language) -> usize {
    let mut x = 170;
    for i in 0..taskbar::MENU_COUNT {
        x += taskbar::menu_label(language, i).chars().count() * FONT_WIDTH + 16;
    }
    x
}

pub fn status(layout: Layout, language: Language, hostname_len: usize) -> StatusLayout {
    let w = layout.width;
    let menu_edge = if w >= 720 { menu_end(language) } else { 165 };
    let free = |left: usize, len: usize| -> Option<Slot> {
        let right = left.checked_add(len)?;
        // Status positions are never drawn on top of interactive menu text.
        if right > w || left < menu_edge.saturating_add(12) { return None; }
        Some(Slot { x: left, width: len })
    };
    let microphone = if w >= 1050 { free(w-424, 82) } else { None };
    let hostname = if w >= 940 && hostname_len <= 12 && hostname_len > 0 {
        free(w-316, hostname_len * FONT_WIDTH)
    } else { None };
    let date = if w >= 840 { free(w-208, 10 * FONT_WIDTH) } else { None };
    StatusLayout {
        microphone, hostname, date,
        time: Slot { x: w.saturating_sub(122), width: 8 * FONT_WIDTH },
        rtc: Slot { x: w.saturating_sub(54), width: 3 * FONT_WIDTH },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_fields_never_overlap_on_supported_viewports() {
        for (w,h) in [(320,240),(640,480),(720,480),(780,600),(800,600),
                      (840,600),(940,600),(1024,768),(1050,700),
                      (1280,800),(1600,900),(1920,1080)] {
            let layout=Layout::for_size(w,h).unwrap();
            for lang in [Language::Ukrainian,Language::English,Language::Russian] {
                let s=status(layout,lang,8);
                let items=[s.microphone,s.hostname,s.date,Some(s.time),Some(s.rtc)];
                for (i,a) in items.iter().enumerate() {
                    if let Some(a)=a {
                        assert!(a.ends_at()<=w);
                        for b in items.iter().skip(i+1).flatten() {
                            assert!(a.ends_at()<=b.x || b.ends_at()<=a.x,
                                "overlap at width {w}: {a:?} {b:?}");
                        }
                    }
                }
                for a in [s.microphone,s.hostname,s.date].into_iter().flatten() {
                    assert!(a.x>=menu_end(lang).saturating_add(12) || w<720);
                }
            }
        }
    }

    #[test]
    fn date_and_host_are_disjoint_even_with_maximum_hostname_length() {
        for w in [940,1024,1050,1280,1600,1920] {
            let s=status(Layout::for_size(w,768).unwrap(),
                         Language::Ukrainian,12);
            let date=s.date.unwrap();
            let host=s.hostname.unwrap();
            assert!(host.ends_at()+10<=date.x);
            assert!(date.ends_at()+10<=s.time.x);
            if let Some(mic)=s.microphone {
                assert!(mic.ends_at()+10<=host.x);
            }
        }
        assert!(status(Layout::for_size(1280,800).unwrap(),
                       Language::English,13).hostname.is_none());
    }
}
