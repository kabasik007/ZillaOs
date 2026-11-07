#![no_std]

// Pure boot-time memory algorithms share one implementation between
// bare-metal kernel and host-side unit tests.
#[path = "kernel/memory.rs"]
pub mod memory;

#[path = "kernel/paging.rs"]
pub mod paging;

#[path = "kernel/page_table_walk.rs"]
pub mod page_table_walk;

#[path = "kernel/process/mod.rs"]
pub mod process;

#[path = "kernel/capabilities/mod.rs"]
pub mod capabilities;

#[path = "kernel/ipc/mod.rs"]
pub mod ipc;

#[path = "kernel/exceptions.rs"]
pub mod exceptions;

pub mod apps;
pub mod compat;
#[path = "shell/desktop.rs"]
pub mod desktop;
pub mod fs;
#[path = "fs/gpt.rs"]
pub mod gpt;
#[path = "fs/install_plan.rs"]
pub mod install_plan;
pub mod loaders;
#[path = "shell/locale.rs"]
pub mod locale;
#[path = "drivers/mouse.rs"]
pub mod mouse;
#[path = "ui/raster.rs"]
pub mod raster;
pub mod services;
pub mod setup;
pub mod shell;
#[path = "drivers/storage_pci.rs"]
pub mod storage_pci;
pub mod video;
#[path = "shell/window_manager.rs"]
pub mod window_manager;

pub mod platform;
