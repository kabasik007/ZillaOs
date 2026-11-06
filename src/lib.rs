#![no_std]

// Pure boot-time memory algorithms share one implementation between
// bare-metal kernel and host-side unit tests.
pub mod memory;

pub mod paging;

pub mod exceptions;

#[path = "shell/desktop.rs"]
pub mod desktop;
#[path = "shell/window_manager.rs"]
pub mod window_manager;
pub mod mouse;
pub mod raster;
pub mod setup;
#[path = "shell/locale.rs"]
pub mod locale;
pub mod fs;
pub mod shell;
pub mod apps;
pub mod install_plan;
pub mod storage_pci;
pub mod gpt;
