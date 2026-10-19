//! Minimal polling i8042 keyboard input for a QEMU q35 guest.
//! Relies on BIOS/OVMF-initialized translated set-1 scan codes.
//! Never enables interrupts and never assumes PS/2 exists on native PCs.
//! Production input must be a separate driver service.

use core::arch::asm;
use zillaos_kernel::desktop::Key;

pub struct Keyboard {
    extended: bool,
}

impl Keyboard {
    pub const fn new() -> Self {
        Self { extended: false }
    }

    pub fn poll(&mut self) -> Option<Key> {
        // SAFETY: i8042 I/O ports exist in the supported QEMU x86-64 target.
        // Status polling does not reconfigure the controller or IRQ masks.
        let status = unsafe { inb(0x64) };
        if status & 1 == 0 {
            return None;
        }
        // Read exactly once to consume the byte, even if it is AUX/unknown.
        let byte = unsafe { inb(0x60) };
        // Bit 5 marks data received from PS/2 auxiliary (mouse) device.
        if status & 0x20 != 0 {
            self.extended = false;
            return None;
        }
        self.decode_set1(byte)
    }

    pub fn decode_set1(&mut self, code: u8) -> Option<Key> {
        if code == 0xe0 {
            self.extended = true;
            return None;
        }
        // Key release and scan-set prefixes should never change GUI state.
        if code & 0x80 != 0 {
            self.extended = false;
            return None;
        }
        if self.extended {
            self.extended = false;
            return None;
        }
        match code {
            0x3b => Some(Key::Launcher),     // F1
            0x3c => Some(Key::Files),        // F2
            0x3d => Some(Key::Terminal),     // F3
            0x3e => Some(Key::Settings),     // F4
            0x3f => Some(Key::ToggleTheme),  // F5
            0x0f => Some(Key::NextApp),      // Tab
            0x01 => Some(Key::Escape),       // Escape
            _ => None,
        }
    }
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let byte: u8;
    // SAFETY: privileged x86-64 port read with no Rust memory side effects.
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") byte,
            options(nomem, nostack, preserves_flags)
        );
    }
    byte
}
