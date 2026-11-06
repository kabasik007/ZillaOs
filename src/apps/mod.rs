//! One installed-app registry for Start, Dock, App Center and launch service.
pub mod id;
pub mod app_center;
pub use id::App;
use crate::locale::TextId;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppDescriptor {
    pub id: App,
    pub name: &'static str,
    pub install_path: &'static str,
    pub title_key: TextId,
    pub glyph: &'static str,
    pub color: u32,
}
pub static BUILTIN_APPS: [AppDescriptor; App::COUNT] = [
    AppDescriptor { id: App::Files, name: "Zilla Files", install_path: "/Program Files/Zilla Files",
                    title_key: TextId::Files, glyph: "F", color: 0x18a9c1 },
    AppDescriptor { id: App::Terminal, name: "Zilla Terminal", install_path: "/Program Files/Zilla Terminal",
                    title_key: TextId::Terminal, glyph: "T", color: 0x425d81 },
    AppDescriptor { id: App::Settings, name: "Zilla Settings", install_path: "/Program Files/Zilla Settings",
                    title_key: TextId::Settings, glyph: "S", color: 0x7d6bd1 },
    AppDescriptor { id: App::AppCenter, name: "Zilla App Center", install_path: "/Program Files/Zilla App Center",
                    title_key: TextId::AppCenter, glyph: "A", color: 0x419776 },
];
pub fn descriptor(id: App) -> &'static AppDescriptor { &BUILTIN_APPS[id.index()] }
pub fn find_by_path(path: &str) -> Option<&'static AppDescriptor> {
    BUILTIN_APPS.iter().find(|x| x.install_path == path)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{lookup, node, NodeKind};
    #[test]
    fn every_registered_app_has_unique_identity_and_virtual_directory() {
        for (i, app) in App::ALL.iter().enumerate() {
            let d = descriptor(*app);
            assert_eq!(d.id, *app);
            assert_eq!(app.index(), i);
            assert_eq!(find_by_path(d.install_path), Some(d));
            assert_eq!(node(lookup(d.install_path).unwrap()).unwrap().kind, NodeKind::Directory);
        }
        assert_eq!(find_by_path("/Windows/unknown.exe"), None);
    }
}
