//! Reusable no_std shell interactions, separate from the boot renderer.
//! The legacy desktop/window-manager code will migrate here incrementally.
pub mod files_interaction;

pub mod clock;

pub mod desktop_icons;

pub mod start_menu;
pub mod taskbar;
pub mod task_view;
