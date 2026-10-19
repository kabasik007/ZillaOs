#![no_std]
#![no_main]

// ZillaOS milestone M1: boot protocol validation, serial diagnostics,
// deterministic success/failure exit for QEMU. Not yet a memory manager or OS.

mod limine;
mod serial;

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    serial::init();
    serial::write("ZillaOS v0.0.1 | x86-64 | Limine\n");

    match limine::memory_region_count() {
        Ok(count) => {
            serial::write("ZILLAOS_MEMMAP_REGIONS=");
            serial::write_hex(count);
            serial::write("\nZILLAOS_BOOT_OK\n");
            serial::debug_exit(0x10);
        }
        Err(error) => {
            match error {
                limine::BootError::UnsupportedRevision => serial::write("ERR: unsupported Limine base revision\n"),
                limine::BootError::MissingMemoryMap => serial::write("ERR: no Limine memory map response\n"),
                limine::BootError::InvalidMemoryMap => serial::write("ERR: invalid memory map metadata\n"),
            }
            serial::write("ZILLAOS_BOOT_FAIL\n");
            serial::debug_exit(0x11);
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    serial::write("ZILLAOS_PANIC\n");
    serial::debug_exit(0x11);
}
