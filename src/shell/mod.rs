//! Reusable no_std shell interactions, separate from the boot renderer.
//! The legacy desktop/window-manager code will migrate here incrementally.
pub mod files_interaction;

pub mod clock;

pub mod desktop_icons;

pub mod start_menu;
pub mod status_bar;
pub mod task_view;
pub mod taskbar;

/// Volatile wallpaper, accent and desktop context-menu geometry.
pub mod personalization;

/// Strict theme profile codec for volatile Documents RAMFS.
pub mod theme_profile;
