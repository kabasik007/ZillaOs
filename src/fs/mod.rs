//! First safe virtual filesystem layer for the development desktop.
//!
//! This is a fixed read-only directory tree in kernel memory. There are no
//! block drivers, host filesystem access, persistence or disk writes.
//! Paths and names are deliberately stable (like Windows Program Files and
//! Users) regardless of the UI locale.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind { Directory, File }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: &'static str,
    pub path: &'static str,
    pub parent: usize,
    pub kind: NodeKind,
    pub data: &'static [u8],
}

pub const ROOT: usize = 0;

// Every parent index must refer to an existing directory. Root is its own
// sentinel parent. These are virtual paths INSIDE ZillaOS, never Windows paths.
pub static NODES: [Node; 25] = [
    Node { name: "ZillaOS", path: "/", parent: 0, kind: NodeKind::Directory, data: b"" },
    Node { name: "Program Files", path: "/Program Files", parent: 0, kind: NodeKind::Directory, data: b"" },
    Node { name: "Users", path: "/Users", parent: 0, kind: NodeKind::Directory, data: b"" },
    Node { name: "System", path: "/System", parent: 0, kind: NodeKind::Directory, data: b"" },
    Node { name: "Temp", path: "/Temp", parent: 0, kind: NodeKind::Directory, data: b"" },
    Node { name: "Zilla Files", path: "/Program Files/Zilla Files", parent: 1, kind: NodeKind::Directory, data: b"" },
    Node { name: "Zilla Terminal", path: "/Program Files/Zilla Terminal", parent: 1, kind: NodeKind::Directory, data: b"" },
    Node { name: "Zilla Settings", path: "/Program Files/Zilla Settings", parent: 1, kind: NodeKind::Directory, data: b"" },
    Node { name: "Guest", path: "/Users/Guest", parent: 2, kind: NodeKind::Directory, data: b"" },
    Node { name: "Public", path: "/Users/Public", parent: 2, kind: NodeKind::Directory, data: b"" },
    Node { name: "Desktop", path: "/Users/Guest/Desktop", parent: 8, kind: NodeKind::Directory, data: b"" },
    Node { name: "Documents", path: "/Users/Guest/Documents", parent: 8, kind: NodeKind::Directory, data: b"" },
    Node { name: "Downloads", path: "/Users/Guest/Downloads", parent: 8, kind: NodeKind::Directory, data: b"" },
    Node { name: "Pictures", path: "/Users/Guest/Pictures", parent: 8, kind: NodeKind::Directory, data: b"" },
    Node { name: "Music", path: "/Users/Guest/Music", parent: 8, kind: NodeKind::Directory, data: b"" },
    Node { name: "Videos", path: "/Users/Guest/Videos", parent: 8, kind: NodeKind::Directory, data: b"" },
    Node { name: "Documents", path: "/Users/Public/Documents", parent: 9, kind: NodeKind::Directory, data: b"" },
    Node { name: "Drivers", path: "/System/Drivers", parent: 3, kind: NodeKind::Directory, data: b"" },
    Node { name: "Fonts", path: "/System/Fonts", parent: 3, kind: NodeKind::Directory, data: b"" },
    Node { name: "Config", path: "/System/Config", parent: 3, kind: NodeKind::Directory, data: b"" },
    Node { name: "Logs", path: "/System/Logs", parent: 3, kind: NodeKind::Directory, data: b"" },
    Node { name: "Cache", path: "/Temp/Cache", parent: 4, kind: NodeKind::Directory, data: b"" },
    Node { name: "WELCOME.TXT", path: "/Users/Guest/Documents/WELCOME.TXT", parent: 11, kind: NodeKind::File,
           data: b"ZillaOS development desktop\nRead-only virtual file\nNothing has been written to a disk." },
    Node { name: "ABOUT.TXT", path: "/System/ABOUT.TXT", parent: 3, kind: NodeKind::File,
           data: b"ZillaOS / Rust / x86-64 / UEFI\nExperimental RAM-only filesystem tree." },
    Node { name: "Zilla App Center", path: "/Program Files/Zilla App Center", parent: 1,
           kind: NodeKind::Directory, data: b"" },
];

pub fn node(id: usize) -> Option<&'static Node> { NODES.get(id) }

pub fn child_at(parent: usize, index: usize) -> Option<(usize, &'static Node)> {
    if node(parent)?.kind != NodeKind::Directory { return None; }
    NODES.iter().enumerate().filter(|(id, n)| *id != ROOT && n.parent == parent).nth(index)
}

pub fn child_count(parent: usize) -> usize {
    if node(parent).is_none_or(|n| n.kind != NodeKind::Directory) { return 0; }
    NODES.iter().enumerate().filter(|(id, n)| *id != ROOT && n.parent == parent).count()
}

/// Reject relative paths, path traversal, empty interior components and
/// unexpected backslashes. Do not depend on locale, disk or heap allocation.
pub fn lookup(path: &str) -> Option<usize> {
    if path == "/" { return Some(ROOT); }
    if !path.starts_with('/') || path.len() > 255 || path.ends_with('/') {
        return None;
    }
    let mut current = ROOT;
    for part in path[1..].split('/') {
        if part.is_empty() || part == "." || part == ".."
            || part.contains('\\') || part.contains('\0') {
            return None;
        }
        current = NODES.iter().enumerate()
            .find(|(id, n)| *id != ROOT && n.parent == current && n.name == part)
            .map(|(id, _)| id)?;
    }
    Some(current)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Browser {
    pub current: usize,
    pub preview_file: Option<usize>,
}

impl Browser {
    pub const fn new() -> Self {
        Self { current: ROOT, preview_file: None }
    }

    pub fn home(&mut self) -> bool {
        let next = Self::new();
        let changed = *self != next;
        *self = next;
        changed
    }

    pub fn up(&mut self) -> bool {
        if self.preview_file.take().is_some() { return true; }
        if self.current == ROOT { return false; }
        let Some(n) = node(self.current) else { return self.home(); };
        self.current = n.parent;
        true
    }

    pub fn open_child(&mut self, row: usize) -> bool {
        let Some((id, entry)) = child_at(self.current, row) else { return false; };
        match entry.kind {
            NodeKind::Directory => {
                self.current = id;
                self.preview_file = None;
            },
            NodeKind::File => self.preview_file = Some(id),
        }
        true
    }

    pub fn preview(&self) -> Option<&'static [u8]> {
        let id = self.preview_file?;
        let entry = node(id)?;
        if entry.kind != NodeKind::File || entry.parent != self.current {
            return None;
        }
        Some(entry.data)
    }
}

impl Default for Browser {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_nodes_have_valid_unique_absolute_paths_and_directory_parents() {
        assert_eq!(lookup("/"), Some(ROOT));
        assert_eq!(NODES[ROOT].parent, ROOT);
        for (id, entry) in NODES.iter().enumerate() {
            assert_eq!(lookup(entry.path), Some(id), "invalid index or path");
            if id != ROOT {
                assert!(entry.path.starts_with('/'));
                assert_eq!(node(entry.parent).unwrap().kind, NodeKind::Directory);
                assert_ne!(entry.parent, id);
            }
        }
    }

    #[test]
    fn stable_program_files_and_user_profile_paths() {
        assert!(lookup("/Program Files/Zilla Files").is_some());
        assert!(lookup("/Program Files/Zilla Terminal").is_some());
        assert!(lookup("/Users/Guest/Desktop").is_some());
        assert!(lookup("/Users/Guest/Documents/WELCOME.TXT").is_some());
        assert!(lookup("/System/Drivers").is_some());
        assert_eq!(child_count(ROOT), 4);
        assert_eq!(child_count(8), 6);
    }

    #[test]
    fn invalid_paths_never_escape_the_virtual_tree() {
        for path in ["", "Users", "../System", "/../System", "/Users//Guest",
            "/Users/Guest/../System", "/Users/Guest/", "/Users\\Guest",
            "/Windows", "/Program Files/Zilla Files/.."] {
            assert_eq!(lookup(path), None, "accepted unsafe path");
        }
        assert_eq!(child_at(22, 0), None);
        assert_eq!(child_count(1000), 0);
    }

    #[test]
    fn browser_navigates_and_previews_without_mutating_the_tree() {
        let mut b = Browser::new();
        assert!(!b.up());
        assert!(!b.open_child(99));
        assert!(b.open_child(1)); // Users
        assert_eq!(b.current, 2);
        assert!(b.open_child(0)); // Guest
        assert!(b.open_child(1)); // Documents
        assert_eq!(node(b.current).unwrap().path, "/Users/Guest/Documents");
        assert!(b.open_child(0)); // WELCOME.TXT
        assert!(core::str::from_utf8(b.preview().unwrap()).unwrap().contains("Read-only"));
        assert!(b.up()); // dismiss preview
        assert!(b.preview().is_none());
        assert!(b.up()); // return Guest
        assert_eq!(node(b.current).unwrap().path, "/Users/Guest");
        assert!(b.home());
        assert_eq!(b.current, ROOT);
        assert!(!b.home());
    }
}
