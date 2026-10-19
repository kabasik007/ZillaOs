#![no_std]
#![no_main]

// M2 integration: CPUID physical-address width, bounded Limine map,
// exclusive 4KiB frame reservations, HHDM pattern/readback/zero probe.

mod cpu;
mod framebuffer;
mod limine;
mod memtest;
mod serial;

use core::panic::PanicInfo;
use zillaos_kernel::memory::FrameAllocator;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    serial::init();
    serial::write("ZillaOS v0.0.2-dev | x86-64 | Limine\n");

    let cpu = match cpu::detect() {
        Ok(info) => info,
        Err(_) => {
            serial::write("ERR: unsupported or invalid x86-64 CPUID\n");
            return_boot_failure();
        }
    };
    serial::write("ZILLAOS_CPU_PHYSICAL_BITS=");
    serial::write_hex(cpu.physical_address_bits as u64);
    serial::write("\nZILLAOS_CPU_VIRTUAL_BITS=");
    serial::write_hex(cpu.virtual_address_bits as u64);
    serial::write("\n");
    if cpu.nx_supported {
        serial::write("ZILLAOS_CPU_NX_SUPPORTED\n");
    }
    serial::write("ZILLAOS_CPU_READY\n");

    let map = match limine::memory_map() {
        Ok(map) => map,
        Err(_) => {
            serial::write("ERR: no valid Limine memory map\n");
            return_boot_failure();
        }
    };
    serial::write("ZILLAOS_MEMMAP_REGIONS=");
    serial::write_hex(map.count as u64);
    serial::write("\n");

    let mut allocator = match FrameAllocator::from_regions_with_physical_bits(
        &map.regions[..map.count],
        cpu.physical_address_bits,
    ) {
        Ok(allocator) => allocator,
        Err(_) => {
            serial::write("ERR: unsafe or invalid physical memory map\n");
            return_boot_failure();
        }
    };
    serial::write("ZILLAOS_USABLE_FRAMES=");
    serial::write_hex(allocator.remaining_frames());
    serial::write("\n");

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
    serial::write("\n");

    let offset = match limine::hhdm_offset() {
        Ok(offset) => offset,
        Err(_) => {
            serial::write("ERR: Limine HHDM missing or invalid\n");
            return_boot_failure();
        }
    };
    serial::write("ZILLAOS_HHDM_OFFSET=");
    serial::write_hex(offset);
    serial::write("\n");

    // SAFETY: allocator returns distinct, exclusively owned USABLE physical
    // pages, and Limine base revision 6 maps USABLE pages via the HHDM.
    if unsafe { memtest::two_frame_probe(first, second, offset, cpu.virtual_address_bits) }.is_err() {
        serial::write("ERR: physical frame HHDM readback mismatch\n");
        return_boot_failure();
    }
    serial::write("ZILLAOS_FRAME_PROBE_OK\n");
    serial::write("ZILLAOS_MEMORY_READY\n");

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
