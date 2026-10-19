//! Minimal 16550-compatible COM1 diagnostics for QEMU x86-64.
//! Hardware I/O is inherently unsafe; it is intentionally confined here.

use core::arch::asm;

const COM1: u16 = 0x3f8;

#[inline]
unsafe fn outb(port: u16, value: u8) {
    // SAFETY: privileged x86-64 port I/O; only used for the known QEMU
    // serial and debug-exit ports during the single-core bootstrap phase.
    unsafe {
        asm!(\n            "out dx, al",\n            in("dx") port,\n            in("al") value,\n            options(nomem, nostack, preserves_flags)\n        );
    }
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let result: u8;
    // SAFETY: reading the QEMU 16550 line status register.
    unsafe {
        asm!(\n            "in al, dx",\n            in("dx") port,\n            out("al") result,\n            options(nomem, nostack, preserves_flags)\n        );
    }
    result
}

pub fn init() {
    // SAFETY: bootstrap runs in ring 0 with interrupts disabled;
    // these writes configure only COM1 (QEMU -serial stdio).
    unsafe {
        outb(COM1 + 1, 0x00); // disable UART interrupts
        outb(COM1 + 3, 0x80); // DLAB
        outb(COM1, 0x03); // 38400 baud divisor low
        outb(COM1 + 1, 0x00); // divisor high
        outb(COM1 + 3, 0x03); // 8N1
        outb(COM1 + 2, 0xc7); // FIFO
        outb(COM1 + 4, 0x0b); // modem control
    }
}

fn write_byte(byte: u8) {
    // If UART is not wired, avoid infinitely hanging a boot attempt.
    let mut attempts = 0_u32;
    while attempts < 100_000 {
        // SAFETY: polling COM1 line status register in ring 0.
        if unsafe { inb(COM1 + 5) } & 0x20 != 0 {
            // SAFETY: write one byte to the configured QEMU serial port.
            unsafe { outb(COM1, byte) };
            return;
        }
        attempts += 1;
        core::hint::spin_loop();
    }
}

pub fn write(message: &str) {
    for byte in message.bytes() {
        if byte == b'\n' {
            write_byte(b'\r');
        }
        write_byte(byte);
    }
}

pub fn write_hex(value: u64) {
    write("0x");
    for shift in (0..16).rev() {
        let digit = ((value >> (shift * 4)) & 0x0f) as u8;
        let ascii = if digit < 10 {\n            b'0' + digit\n        } else {\n            b'a' + digit - 10\n        };
        write_byte(ascii);
    }
}

/// QEMU `isa-debug-exit` exits with process status `(value << 1) | 1`.
/// Value 0x10 = 33 (success), 0x11 = 35 (failure).
pub fn debug_exit(value: u32) -> ! {
    // SAFETY: QEMU-only debug exit test device, never a host disk or MMIO.
    unsafe {
        asm!(\n            "out dx, eax",\n            in("dx") 0xf4_u16,\n            in("eax") value,\n            options(nomem, nostack, preserves_flags)\n        );
    }
    loop {
        // SAFETY: halt idle CPU if debug-exit device is not present.
        unsafe {\n            asm!("cli; hlt", options(nomem, nostack));\n        }
    }
}
