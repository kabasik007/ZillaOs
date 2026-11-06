//! Built-in ZillaOS application catalogue.
//!
//! These entries identify kernel-embedded preview apps and reserve stable
//! virtual installation directories. They are NOT native executables and do
//! not claim process isolation, .exe loading or package installation.
use crate::desktop::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppDescriptor {
    pub id: App,
    pub name: &'static str,
    pub install_path: &'static str,
}

pub const BUILTIN_APPS: [AppDescriptor; 3] = [
    AppDescriptor {
        id: App::Files, name: "Zilla Files",
        install_path: "/Program Files/Zilla Files",
    },
    AppDescriptor {
        id: App::Terminal, name: "Zilla Terminal",
        install_path: "/Program Files/Zilla Terminal",
    },
    AppDescriptor {
        id: App::Settings, name: "Zilla Settings",
        install_path: "/Program Files/Zilla Settings",
    },
];

pub fn find_by_path(path: &str) -> Option<&'static AppDescriptor> {
    BUILTIN_APPS.iter().find(|app| app.install_path == path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{lookup, node, NodeKind};

    #[test]
    fn installed_app_paths_exist_in_guest_tree_not_on_host_disk() {
        for app in BUILTIN_APPS {
            assert_eq!(find_by_path(app.install_path).unwrap().id, app.id);
            assert!(!app.name.is_empty());
            let entry_id = lookup(app.install_path).expect("missing guest directory");
            assert_eq!(node(entry_id).unwrap().kind, NodeKind::Directory);
        }
        assert_eq!(find_by_path("/Users/Guest/Documents"), None);
    }
}
