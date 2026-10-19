//! Minimal single-CPU x86-64 exception diagnostics.
//!
//! Only #UD (6), #GP (13), and #PF (14) are installed. Other IDT entries
//! intentionally remain missing. This is NOT a fully initialized GDT/TSS,
//! interrupt controller, ring-3 interrupt design or recoverable fault handler.
//! We reuse Limine's already-loaded ring-0 code selector for the gate.
//! Hardware interrupts stay disabled throughout this bootstrap.

use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::mem::size_of;
use zillaos_kernel::exceptions::page_fault_flags;

use crate::serial;

#[repr(C)]
#[derive(Clone, Copy)]
struct IdtGate {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtGate {
    const ABSENT: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        attributes: 0,
        offset_middle: 0,
        offset_high: 0,
        reserved: 0,
    };

    fn interrupt(handler: u64, code_selector: u16) -> Self {
        Self {
            offset_low: handler as u16,
            selector: code_selector,
            ist: 0, // No TSS/IST until we own GDT and guarded kernel stacks.
            attributes: 0x8e, // Present, DPL=0, 64-bit interrupt gate.
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

#[repr(C, align(16))]
struct IdtStorage(UnsafeCell<[IdtGate; 256]>);

// SAFETY: exactly one CPU is active in early boot; IDT storage is mutated
// only during installation before LIDT. All later reads are by the CPU.
unsafe impl Sync for IdtStorage {}

static IDT: IdtStorage = IdtStorage(UnsafeCell::new([IdtGate::ABSENT; 256]));

unsafe extern "C" {
    fn zilla_exception_ud();
    fn zilla_exception_gp();
    fn zilla_exception_pf();
}

// The CPU frame at ring 0 is RIP, CS, RFLAGS (for #UD), preceded by an
// error code for #GP and #PF. We capture values before adjusting RSP,
// which is necessary for the C ABI. Fault handlers never return (no IRETQ).
//
// WARNING: Without an IST, stack faults and double faults may triple-fault.
// Those scenarios are deliberately outside the current bootstrap.
global_asm!(
    r#"
    .section .text
    .global zilla_exception_ud
zilla_exception_ud:
    cli
    mov rdi, 6
    xor esi, esi
    mov rdx, [rsp]
    xor ecx, ecx
    and rsp, -16
    call zilla_exception_fatal
1:
    hlt
    jmp 1b

    .global zilla_exception_gp
zilla_exception_gp:
    cli
    mov rdi, 13
    mov rsi, [rsp]
    mov rdx, [rsp + 8]
    xor ecx, ecx
    and rsp, -16
    call zilla_exception_fatal
2:
    hlt
    jmp 2b

    .global zilla_exception_pf
zilla_exception_pf:
    cli
    mov rdi, 14
    mov rsi, [rsp]
    mov rdx, [rsp + 8]
    mov rcx, cr2
    and rsp, -16
    call zilla_exception_fatal
3:
    hlt
    jmp 3b
    "#
);

pub fn install() -> Result<(), &'static str> {
    let selector: u16;
    // SAFETY: reading CS is side-effect free on x86-64.
    unsafe {
        asm!(
            "mov {0:x}, cs",
            out(reg) selector,
            options(nomem, nostack, preserves_flags)
        );
    }
    if selector == 0 || selector & 0x3 != 0 {
        return Err("kernel CS is not a ring-0 selector");
    }

    let table_ptr = IDT.0.get();
    // SAFETY: boot BSP is the only active CPU. No IDT was installed by
    // ZillaOS yet. Writes complete before the new IDTR becomes active.
    unsafe {
        (*table_ptr)[6] = IdtGate::interrupt(zilla_exception_ud as *const () as u64, selector);
        (*table_ptr)[13] = IdtGate::interrupt(zilla_exception_gp as *const () as u64, selector);
        (*table_ptr)[14] = IdtGate::interrupt(zilla_exception_pf as *const () as u64, selector);
    }

    let descriptor = IdtPointer {
        limit: (size_of::<[IdtGate; 256]>() - 1) as u16,
        base: table_ptr as u64,
    };
    // SAFETY: IDT is a static 16-byte aligned 256-entry table; descriptor
    // references valid kernel memory, and interrupts remain masked.
    unsafe {
        asm!(
            "lidt [{}]",
            in(reg) &descriptor,
            options(readonly, nostack, preserves_flags)
        );
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn zilla_exception_fatal(
    vector: u64,
    error: u64,
    rip: u64,
    cr2: u64,
) -> ! {
    serial::write("ZILLAOS_EXCEPTION_VECTOR=");
    serial::write_hex(vector);
    serial::write("\nZILLAOS_EXCEPTION_ERROR=");
    serial::write_hex(error);
    serial::write("\nZILLAOS_EXCEPTION_RIP=");
    serial::write_hex(rip);

    if vector == 14 {
        serial::write("\nZILLAOS_EXCEPTION_CR2=");
        serial::write_hex(cr2);
        let flags = page_fault_flags(error);
        if flags.write {
            serial::write("\nZILLAOS_EXCEPTION_WRITE");
        }
        if flags.user {
            serial::write("\nZILLAOS_EXCEPTION_USER");
        }
        if flags.instruction_fetch {
            serial::write("\nZILLAOS_EXCEPTION_EXECUTE");
        }
    }
    serial::write("\nZILLAOS_EXCEPTION_FATAL\n");
    serial::debug_exit(0x11);
}
