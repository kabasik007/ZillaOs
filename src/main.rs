#![no_std]
#![no_main]

// M2 integration: CPUID physical-address width, bounded Limine map,
// exclusive 4KiB frame reservations, HHDM pattern/readback/zero probe.

mod cpu;
mod framebuffer;
#[cfg(feature = "vm-demo")]
mod graphics;
#[cfg(feature = "vm-demo")]
mod input;
mod interrupts;
mod limine;
mod memtest;
mod serial;

use core::panic::PanicInfo;
use zillaos_kernel::memory::FrameAllocator;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    serial::init();
    serial::write("ZillaOS v0.0.2-dev | x86-64 | Limine\n");

    if interrupts::install().is_err() {
        serial::write("ERR: cannot install minimal CPU exception handlers\n");
        return_boot_failure();
    }
    serial::write("ZILLAOS_IDT_READY\n");

    // In the explicit fault-test ISO only: UD2 must enter vector 6 handler.
    // The handler prints diagnostics and exits QEMU with code 35.
    #[cfg(feature = "exception-self-test")]
    unsafe {
        core::arch::asm!("ud2", options(noreturn));
    }

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

    #[cfg(feature = "vm-demo")]
    {
        use zillaos_kernel::desktop::{App, Desktop, Layout};
        use zillaos_kernel::setup::{Wizard, Step};

        let layout = limine::framebuffer()
            .and_then(|fb| Layout::for_size(fb.width, fb.height));
        let mut wizard = Wizard::new(layout.unwrap_or_else(|| {
            Layout::for_size(1024, 768).expect("static fallback viewport")
        }));

        // The first thing a user sees is Setup, not the Desktop.
        // This is a volatile onboarding UI, NOT an installer that
        // partitions a disk, creates accounts or persists config.
        let setup_available = graphics::render_setup(&wizard);
        serial::write("ZILLAOS_SETUP_READY\n");
        if setup_available {
            serial::write("ZILLAOS_FRAMEBUFFER_READY\n");
        } else {
            serial::write("ZILLAOS_SETUP_NO_FRAMEBUFFER\n");
        }
        serial::write("ZILLAOS_BOOT_OK\n");
        serial::write("ZILLAOS_DEMO_READY\n");

        let mut input = input::Input::initialize();
        if input.mouse_available() {
            wizard.enable_pointer();
            serial::write("ZILLAOS_MOUSE_READY\n");
            if setup_available {
                graphics::render_setup(&wizard);
            }
        } else {
            serial::write("ZILLAOS_MOUSE_UNAVAILABLE\n");
        }
        serial::write("ZILLAOS_SETUP_INPUT_POLL\n");

        if setup_available {
            while !wizard.finished() {
                let before = wizard;
                let event = input.poll();
                let was_mouse = matches!(&event, Some(input::InputEvent::Mouse(_)));
                let changed = match event {
                    Some(input::InputEvent::Key(key)) => wizard.handle_key(key),
                    Some(input::InputEvent::Mouse(packet)) => wizard.handle_mouse(packet),
                    None => false,
                };
                if changed {
                    if wizard.step != before.step {
                        serial::write("ZILLAOS_SETUP_STEP=");
                        serial::write_hex(wizard.step.index() as u64);
                        serial::write("\n");
                    }
                    if wizard.finished() {
                        break;
                    }
                    let visual_change = wizard.step != before.step
                        || wizard.language != before.language
                        || wizard.region != before.region
                        || wizard.timezone != before.timezone
                        || wizard.hostname != before.hostname
                        || wizard.dark != before.dark
                        || wizard.selection != before.selection;
                    if !was_mouse || visual_change
                        || !graphics::redraw_setup_cursor(before.pointer, &wizard)
                    {
                        graphics::render_setup(&wizard);
                    }
                }
                core::hint::spin_loop();
            }
        } else {
            // Serial-only fallback is intentionally noninteractive: the
            // user must have a framebuffer to complete graphical setup.
            wizard.step = Step::Finished;
            serial::write("ZILLAOS_SETUP_SKIPPED_NO_FRAMEBUFFER\n");
        }

        serial::write("ZILLAOS_SETUP_COMPLETE\n");
        serial::write("ZILLAOS_SETUP_LANGUAGE=");
        serial::write(wizard.language.code());
        serial::write("\nZILLAOS_SETUP_TIMEZONE=");
        serial::write(wizard.timezone.identifier());
        serial::write("\nZILLAOS_SETUP_HOSTNAME=");
        serial::write(wizard.hostname.as_str());
        serial::write("\nZILLAOS_SETUP_REGION=");
        match wizard.region {
            zillaos_kernel::setup::Region::Ukraine => serial::write("UA\n"),
            zillaos_kernel::setup::Region::Poland => serial::write("PL\n"),
            zillaos_kernel::setup::Region::UnitedStates => serial::write("US\n"),
        }
        serial::write("ZILLAOS_SETUP_THEME=");
        serial::write(if wizard.dark { "DARK\n" } else { "LIGHT\n" });
        serial::write("ZILLAOS_SETUP_VOLATILE_ONLY\n");

        let mut desktop = Desktop::with_layout(layout.unwrap_or_else(|| {
            Layout::for_size(1024, 768).expect("static fallback viewport")
        }));
        desktop.dark = wizard.dark;
        desktop.hostname = wizard.hostname;
        desktop.language = wizard.language;
        desktop.region = wizard.region;
        desktop.timezone = wizard.timezone;
        if input.mouse_available() {
            desktop.enable_pointer();
        }

        let desktop_available = graphics::render(&desktop);
        if desktop_available {
            if graphics::has_backbuffer() {
                serial::write("ZILLAOS_GRAPHICS_BUFFERED_READY\n");
            } else {
                serial::write("ZILLAOS_GRAPHICS_DIRECT_FALLBACK\n");
            }
            serial::write("ZILLAOS_DESKTOP_READY\n");
        } else {
            serial::write("ZILLAOS_DESKTOP_NO_FRAMEBUFFER\n");
        }

        serial::write("ZILLAOS_DESKTOP_INPUT_POLL\n");
        serial::write("ZILLAOS_WINDOW_MANAGER_READY\n");
        let mut first_mouse_motion_logged = false;
        let mut first_cursor_damage_logged = false;

        // Desktop stays an early single-core test environment.
        loop {
            let previous_windows = desktop.windows;
            let previous_launcher = desktop.launcher;
            let previous_theme = desktop.dark;
            let previous_pointer = desktop.pointer;
            let event = input.poll();
            let mouse_event = matches!(&event, Some(input::InputEvent::Mouse(_)));
            let changed = match event {
                Some(input::InputEvent::Key(key)) => desktop.handle(key),
                Some(input::InputEvent::Mouse(packet)) => desktop.handle_mouse(packet),
                None => false,
            };
            if changed {
                if mouse_event {
                    if !first_mouse_motion_logged
                        && (previous_pointer.x != desktop.pointer.x
                            || previous_pointer.y != desktop.pointer.y)
                    {
                        first_mouse_motion_logged = true;
                        serial::write("ZILLAOS_MOUSE_MOTION_DETECTED\n");
                    }
                    if !previous_pointer.left && desktop.pointer.left {
                        serial::write("ZILLAOS_MOUSE_LEFT_DOWN\n");
                    } else if previous_pointer.left && !desktop.pointer.left {
                        serial::write("ZILLAOS_MOUSE_LEFT_UP\n");
                    }
                }
                if previous_windows != desktop.windows
                    || previous_launcher != desktop.launcher
                    || previous_theme != desktop.dark
                {
                    serial::write("ZILLAOS_DESKTOP_STATE_CHANGED\n");
                    serial::write("ZILLAOS_DESKTOP_VISIBLE=");
                    serial::write_hex(desktop.windows.visible_count() as u64);
                    serial::write("\nZILLAOS_DESKTOP_FOCUS=");
                    match desktop.active() {
                        Some(App::Files) => serial::write("FILES\n"),
                        Some(App::Terminal) => serial::write("TERMINAL\n"),
                        Some(App::Settings) => serial::write("SETTINGS\n"),
                        None => serial::write("NONE\n"),
                    }
                }
                if desktop_available {
                    let scene_changed = previous_windows != desktop.windows
                        || previous_launcher != desktop.launcher
                        || previous_theme != desktop.dark;
                    let cursor_damage_applied = mouse_event
                        && !scene_changed
                        && graphics::redraw_cursor(previous_pointer, &desktop);
                    if cursor_damage_applied {
                        if !first_cursor_damage_logged {
                            first_cursor_damage_logged = true;
                            serial::write("ZILLAOS_GRAPHICS_CURSOR_DAMAGE_OK\n");
                        }
                    } else {
                        graphics::render(&desktop);
                    }
                }
            }
            core::hint::spin_loop();
        }
    }

    #[cfg(not(feature = "vm-demo"))]
    {
        if framebuffer::render() {
            serial::write("ZILLAOS_FRAMEBUFFER_READY\n");
        } else {
            serial::write("ZILLAOS_FRAMEBUFFER_UNAVAILABLE\n");
        }
        serial::write("ZILLAOS_BOOT_OK\n");
        serial::debug_exit(0x10);
    }
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
