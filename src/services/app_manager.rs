//! One safe launch gateway for all ZillaOS shell surfaces.
//! WindowManager is the only owner of window state. We do not create
//! processes or execute any foreign code in this preview.
use crate::apps::{self, App};
use crate::window_manager::WindowManager;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchError {
    NotInstalled,
    RuntimeUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchTarget<'a> {
    Builtin(App),
    InstalledPath(&'a str),
    ForeignBinary(&'a [u8]),
}

pub struct ApplicationManager;
impl ApplicationManager {
    pub fn launch(windows: &mut WindowManager, target: LaunchTarget<'_>)
        -> Result<App, LaunchError> {
        let app = match target {
            LaunchTarget::Builtin(id) => id,
            LaunchTarget::InstalledPath(path) => apps::find_by_path(path)
                .map(|descriptor| descriptor.id).ok_or(LaunchError::NotInstalled)?,
            LaunchTarget::ForeignBinary(_) => return Err(LaunchError::RuntimeUnavailable),
        };
        if apps::descriptor(app).id != app { return Err(LaunchError::NotInstalled); }
        windows.open(app);
        Ok(app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::Layout;
    use crate::window_manager::WindowMode;
    fn windows() -> WindowManager {
        WindowManager::new(Layout::for_size(1024, 768).unwrap())
    }
    #[test]
    fn start_dock_and_installed_path_use_one_open_restore_flow() {
        let mut w = windows();
        assert_eq!(ApplicationManager::launch(&mut w,
                   LaunchTarget::Builtin(App::AppCenter)), Ok(App::AppCenter));
        w.minimize_focused();
        assert_eq!(w.window(App::AppCenter).mode, WindowMode::Minimized);
        assert_eq!(ApplicationManager::launch(&mut w,
                   LaunchTarget::InstalledPath("/Program Files/Zilla App Center")),
                   Ok(App::AppCenter));
        assert_eq!(w.window(App::AppCenter).mode, WindowMode::Normal);
        assert_eq!(w.focused(), Some(App::AppCenter));
    }
    #[test]
    fn foreign_programs_and_unknown_paths_fail_closed() {
        let mut w = windows();
        let previous = w;
        for payload in [b"MZ".as_slice(), b"\x7fELF".as_slice(), b"PK\x03\x04".as_slice()] {
            assert_eq!(ApplicationManager::launch(&mut w,
                       LaunchTarget::ForeignBinary(payload)), Err(LaunchError::RuntimeUnavailable));
        }
        assert_eq!(ApplicationManager::launch(&mut w,
                   LaunchTarget::InstalledPath("/Windows/system32/app.exe")),
                   Err(LaunchError::NotInstalled));
        assert_eq!(previous, w);
    }
    #[test]
    fn every_installed_builtin_uses_the_same_registry() {
        let mut w = windows();
        for descriptor in apps::BUILTIN_APPS.iter() {
            assert_eq!(ApplicationManager::launch(&mut w,
                       LaunchTarget::InstalledPath(descriptor.install_path)), Ok(descriptor.id));
        }
        assert_eq!(w.visible_count(), App::COUNT);
    }
}
