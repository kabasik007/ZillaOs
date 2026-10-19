#![no_std]
#![no_main]

// M1: independent Rust kernel booting through Limine.
// QEMU test mode exits automatically; VM demo mode draws a splash and halts.

mod framebuffer;
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
            serial::write("\n");
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
                    // SAFETY: no interrupts have been enabled at this early stage.
                    // Remain in QEMU so that the graphical splash stays visible.
                    unsafe {
                        core::arch::asm!("cli; hlt", options(nomem, nostack));
                    }
                }
            }
            #[cfg(not(feature = "vm-demo"))]
            serial::debug_exit(0x10);
        }
        Err(error) => {
            match error {
                limine::BootError::UnsupportedRevision => {
                    serial::write("ERR: unsupported Limine base revision\n");
                }
                limine::BootError::MissingMemoryMap => {
                    serial::write("ERR: no Limine memory map response\n");
                }
                limine::BootError::InvalidMemoryMap => {
                    serial::write("ERR: invalid memory map metadata\n");
                }
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
