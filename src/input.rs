//! Poll translated scan-set-1 bytes from QEMU's i8042 keyboard port.
//! A single CPU polls without enabling IRQs; native USB input unsupported.

use core::arch::asm;
use zillaos_kernel::desktop::{Key, ScancodeDecoder};

pub struct Keyboard {
    decoder: ScancodeDecoder,
}

impl Keyboard {
    pub const fn new() -> Self {
        Self {
            decoder: ScancodeDecoder::new(),
        }
    }

    pub fn poll(&mut self) -> Option<Key> {
        // SAFETY: x86-64 privileged I/O ports of QEMU i8042 controller.
        let status = unsafe { inb(0x64) };
        if status & 1 == 0 {
            return None;
        }
        // Consume exactly one byte regardless of whether it came from
        // keyboard or auxiliary port, to avoid an input queue stall.
        let value = unsafe { inb(0x60) };
        if status & 0x20 != 0 {
            self.decoder.reset();
            return None;
        }
        self.decoder.feed(value)
    }
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: hardware port read; no pointers or Rust-owned memory touched.
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
