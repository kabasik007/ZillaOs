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
pub mod services;
pub mod compat;
pub mod loaders;
pub mod video;
#[path = "fs/install_plan.rs"]
pub mod install_plan;
#[path = "drivers/storage_pci.rs"]
pub mod storage_pci;
#[path = "fs/gpt.rs"]
pub mod gpt;

// Unified read-only PCI discovery and caller-owned device registry.
pub mod platform;
