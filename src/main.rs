#![no_std]
#![no_main]

// Kernel milestone M1 + early M2: validate Limine memory map and reserve
// two distinct physical 4 KiB frames, without mapping or writing to them.

mod framebuffer;
mod limine;
mod serial;

use core::panic::PanicInfo;
use zillaos_kernel::memory::FrameAllocator;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    serial::init();
    serial::write("ZillaOS v0.0.1 | x86-64 | Limine\n");

    let map = match limine::memory_map() {
        Ok(map) => map,
        Err(error) => {
            match error {
                limine::BootError::UnsupportedRevision => {
                    serial::write("ERR: unsupported Limine base revision\n");
                }
                limine::BootError::MissingMemoryMap => {
                    serial::write("ERR: no Limine memory map response\n");
                }
                limine::BootError::InvalidMemoryMap => {
                    serial::write("ERR: invalid Limine memory map metadata\n");
                }
            }
            return_boot_failure();
        }
    };

    serial::write("ZILLAOS_MEMMAP_REGIONS=");
    serial::write_hex(map.count as u64);
    serial::write("\n");

    let mut allocator = match FrameAllocator::from_regions(&map.regions[..map.count]) {
        Ok(allocator) => allocator,
        Err(_error) => {
            serial::write("ERR: invalid/overlapping/empty physical memory map\n");
            return_boot_failure();
        }
    };

    serial::write("ZILLAOS_USABLE_FRAMES=");
    serial::write_hex(allocator.remaining_frames());
    serial::write("\n");

    // The early bump allocator can *reserve* addresses, not yet map pages.
    // Retain it until shutdown; its frames are not returned to other code.
    let first = match allocator.reserve_frame() {
        Some(frame) => frame,
        None => return_boot_failure(),
    };
    let second = match allocator.reserve_frame() {
        Some(frame) if frame.address != first.address => frame,
        _ => return_boot_failure(),
    };

    serial::write("ZILLAOS_TEST_FRAME_1=");
    serial::write_hex(first.address);
    serial::write("\nZILLAOS_TEST_FRAME_2=");
    serial::write_hex(second.address);
    serial::write("\nZILLAOS_MEMORY_READY\n");

    if framebuffer::render() {
        serial::write("ZILLAOS_FRAMEBUFFER_READY\n");
    } else {
        serial::write("ZILLAOS_FRAMEBUFFER_UNAVAILABLE\n");
    }

    serial::write("ZILLAOS_BOOT_OK\n");
    #[cfg(feature = "vm-demo")]
    {
        serial::write("ZILLAOS_DEMO_READY\n");
        loop {
            // SAFETY: interrupts have not been enabled in this bootstrap.
            unsafe {
                core::arch::asm!("cli; hlt", options(nomem, nostack));
            }
        }
    }
    #[cfg(not(feature = "vm-demo"))]
    serial::debug_exit(0x10);
}

#[cold]
fn return_boot_failure() -> ! {
    serial::write("ZILLAOS_BOOT_FAIL\n");
    serial::debug_exit(0x11);
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    serial::write("ZILLAOS_PANIC\n");
    serial::debug_exit(0x11);
}
