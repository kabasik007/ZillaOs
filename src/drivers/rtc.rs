//! Read-only CMOS RTC access, x86-64 VMware/OVMF boot preview.
//! No RTC writes or time-setting commands. Single boot CPU and no concurrent
//! CMOS users in this experimental kernel. BIOS hour format is detected.
use core::arch::asm;
use zillaos_kernel::shell::clock::ClockTime;

#[inline]
fn read_cmos(register: u8) -> u8 {
    let mut value: u8;
    // SAFETY: only the early single-core kernel reads the standard x86 CMOS
    // address/data I/O ports. Temporarily mask NMI during each register read.
    unsafe {
        asm!("out dx, al", in("dx") 0x70u16, in("al") register | 0x80,
             options(nomem, nostack, preserves_flags));
        asm!("in al, dx", in("dx") 0x71u16, out("al") value,
             options(nomem, nostack, preserves_flags));
        asm!("out dx, al", in("dx") 0x70u16, in("al") register,
             options(nomem, nostack, preserves_flags));
    }
    value
}

#[inline]
fn update_in_progress() -> bool {
    read_cmos(0x0a) & 0x80 != 0
}

fn snapshot() -> Option<ClockTime> {
    if update_in_progress() { return None; }
    let second = read_cmos(0x00);
    let minute = read_cmos(0x02);
    let hour = read_cmos(0x04);
    let status_b = read_cmos(0x0b);
    if update_in_progress() { return None; }
    ClockTime::from_cmos(hour, minute, second, status_b)
}

/// Return None when RTC is updating/inconsistent or unavailable.
pub fn read_time() -> Option<ClockTime> {
    for _ in 0..3 {
        let Some(a) = snapshot() else { continue };
        if snapshot() == Some(a) { return Some(a); }
    }
    None
}
