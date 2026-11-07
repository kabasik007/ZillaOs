#![no_std]

// Standalone bare-metal compilation gate; never links the real kernel.
// No shared Cargo, module or bootstrap edits are required.
#[path = "../src/services/service_manager.rs"]
pub mod service_manager;
