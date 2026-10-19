#![no_std]

// Pure boot-time memory algorithms share one implementation between
// bare-metal kernel and host-side unit tests.
pub mod memory;

pub mod paging;

pub mod exceptions;

pub mod desktop;
