#![no_std]

// Pure boot-time memory algorithms share one implementation between
// bare-metal kernel and host-side unit tests.
#[path = "kernel/memory.rs"]
pub mod memory;

#[path = "kernel/paging.rs"]
pub mod paging;

#[path = "kernel/exceptions.rs"]
pub mod exceptions;

#[path = "shell/desktop.rs"]
pub mod desktop;
#[path = "shell/window_manager.rs"]
pub mod window_manager;
#[path = "drivers/mouse.rs"]
pub mod mouse;
#[path = "ui/raster.rs"]
pub mod raster;
pub mod setup;
#[path = "shell/locale.rs"]
pub mod locale;
pub mod fs;
pub mod shell;
pub mod apps;
pub mod compat;
#[path = "fs/install_plan.rs"]
pub mod install_plan;
#[path = "drivers/storage_pci.rs"]
pub mod storage_pci;
#[path = "fs/gpt.rs"]
pub mod gpt;
