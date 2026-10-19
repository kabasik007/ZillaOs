//! QEMU i8042 input: keyboard + optional standard 3-byte PS/2 mouse.
//!
//! This is a time-bounded, polling-only bootstrap driver. We intentionally
//! do not enable IRQ1/IRQ12 or install native-machine device drivers.
//! Keyboard events and AUX bytes share port 0x60; NEVER create two
//! independent readers or mouse packets will be lost or misclassified.

use core::arch::asm;
use zillaos_kernel::desktop::{Key, ScancodeDecoder};
use zillaos_kernel::mouse::{MouseDecoder, MousePacket};

const PORT_DATA: u16 = 0x60;
const PORT_STATUS: u16 = 0x64;
const STATUS_OUTPUT_FULL: u8 = 0x01;
const STATUS_INPUT_FULL: u8 = 0x02;
const STATUS_MOUSE: u8 = 0x20;
const POLL_LIMIT: usize = 200_000;

pub enum InputEvent {
    Key(Key),
    Mouse(MousePacket),
}

pub struct Input {
    keyboard: ScancodeDecoder,
    mouse: MouseDecoder,
    mouse_available: bool,
}

impl Input {
    pub fn initialize() -> Self {
        let available = initialize_mouse();
        Self {
            keyboard: ScancodeDecoder::new(),
            mouse: MouseDecoder::new(),
            mouse_available: available,
        }
    }

    pub const fn mouse_available(&self) -> bool {
        self.mouse_available
    }

    /// Consume at most one pending byte per call. Missing PS/2 ports return
    /// None; unknown keyboard bytes cannot masquerade as mouse packets.
    pub fn poll(&mut self) -> Option<InputEvent> {
        // SAFETY: privileged I/O restricted to QEMU i8042 ports.
        let status = unsafe { inb(PORT_STATUS) };
        if status & STATUS_OUTPUT_FULL == 0 {
            return None;
        }
        let byte = unsafe { inb(PORT_DATA) };
        if status & STATUS_MOUSE != 0 {
            if self.mouse_available {
                self.mouse.feed(byte).map(InputEvent::Mouse)
            } else {
                None
            }
        } else {
            self.keyboard.feed(byte).map(InputEvent::Key)
        }
    }
}

fn await_writable() -> bool {
    for _ in 0..POLL_LIMIT {
        if unsafe { inb(PORT_STATUS) } & STATUS_INPUT_FULL == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn command(byte: u8) -> bool {
    if !await_writable() {
        return false;
    }
    // SAFETY: command port of the QEMU i8042 controller.
    unsafe { outb(PORT_STATUS, byte) };
    true
}

fn data(byte: u8) -> bool {
    if !await_writable() {
        return false;
    }
    // SAFETY: QEMU i8042 data port.
    unsafe { outb(PORT_DATA, byte) };
    true
}

fn drain_stale_output() {
    // Bound the flush to avoid a stuck controller hanging the kernel.
    for _ in 0..32 {
        if unsafe { inb(PORT_STATUS) } & STATUS_OUTPUT_FULL == 0 {
            break;
        }
        let _ = unsafe { inb(PORT_DATA) };
    }
}

fn read_controller_config() -> Option<u8> {
    if !command(0x20) { return None; }
    for _ in 0..POLL_LIMIT {
        let status = unsafe { inb(PORT_STATUS) };
        if status & STATUS_OUTPUT_FULL != 0 {
            let byte = unsafe { inb(PORT_DATA) };
            if status & STATUS_MOUSE == 0 {
                return Some(byte);
            }
        }
        core::hint::spin_loop();
    }
    None
}

fn read_mouse_ack() -> bool {
    for _ in 0..POLL_LIMIT {
        let status = unsafe { inb(PORT_STATUS) };
        if status & STATUS_OUTPUT_FULL != 0 {
            let byte = unsafe { inb(PORT_DATA) };
            if status & STATUS_MOUSE != 0 {
                return byte == 0xfa; // Mouse ACK. RESEND is a failure.
            }
        }
        core::hint::spin_loop();
    }
    false
}

fn send_mouse(byte: u8) -> bool {
    // The 0xd4 command routes the next data-port byte to PS/2 AUX.
    command(0xd4) && data(byte) && read_mouse_ack()
}

fn initialize_mouse() -> bool {
    drain_stale_output();
    if !command(0xa8) { // enable controller second PS/2 port
        return false;
    }

    // Read-modify-write rather than destroying keyboard translation config.
    let Some(config) = read_controller_config() else {
        return false;
    };
    // Disable AUX IRQ12 (we poll) and clear AUX clock-disable bit. Preserve
    // all other firmware-defined bits, including set-1 keyboard translation.
    let config = config & !(0x02 | 0x20);
    if !command(0x60) || !data(config) {
        return false;
    }

    // Reset mouse mode to standard PS/2 3-byte packets, then enable streaming.
    // Some devices are absent: a bounded failure leaves keyboard usable.
    send_mouse(0xf6) && send_mouse(0xf4)
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: x86-64 I/O port read without memory access.
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value,
             options(nomem, nostack, preserves_flags));
    }
    value
}

#[inline]
unsafe fn outb(port: u16, value: u8) {
    // SAFETY: x86-64 I/O port write to known controller registers.
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value,
             options(nomem, nostack, preserves_flags));
    }
}
