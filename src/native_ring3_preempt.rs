//! CPU0-only real timer IRQ0 preemption of two controlled CPL3 contexts.
//! Only compiled into the disposable QEMU `ring3-preempt` ISO.
//! This is an 8253/8254 PIT + remapped legacy PIC experiment, not
//! production APIC/SMP scheduling, process isolation across CR3s, or a GUI.
use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::ptr;
use zillaos_kernel::{
    memory::PAGE_SIZE,
    process::{ExitReason, ProcessTable},
    process::address_space::{Permissions, UserRegion},
    process::thread::{ThreadState, ThreadTable, Tid},
};
use crate::{native_ring3::{self, Ring3Error}, native_vm_staging::StagedSpace, serial};

const CODE: u64 = 0x0040_0000;
const STACK_A: u64 = 0x0080_0000;
const STACK_B: u64 = 0x007f_e000;
const ENTRY_B: u64 = CODE + 128;
const SAMPLES: usize = 8;
const EXPECTED: &[u8; SAMPLES] = b"ABABABAB";
const SENTINELS: [u64; 2] = [0x9abc_0001_55aa_0001, 0x9abc_0002_55aa_0002];
const PIT_DIVISOR: u16 = 11932; // ~=100 Hz, legacy PIT input 1_193_182 Hz

// PUSH order in zilla_native_timer_irq, at the bottom of the trap stack:
// RAX,R11,R10,R9,R8,RCX,RDX,RSI,RDI,R15,R14,R13,R12,RBP,RBX.
const GP_RBX: usize = 14;
const GP_R12: usize = 12;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimerFrame {
    gp: [u64; 15],
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

impl TimerFrame {
    const fn initial(entry: u64, stack: u64) -> Self {
        Self {
            gp: [0; 15], rip: entry, cs: 0x23,
            rflags: 0x202, // architectural reserved bit and IF=1
            rsp: stack, ss: 0x1b,
        }
    }
}

struct Runtime {
    threads: ThreadTable,
    tids: [Tid; 2],
    frames: [TimerFrame; 2],
    current: usize,
    ticks: usize,
    trace: [u8; SAMPLES],
    previous_counter: [u64; 2],
    observed: [u64; 2],
    failed: bool,
}

struct CpuCell<T>(UnsafeCell<T>);
// SAFETY: single BSP and no concurrent access; interrupt gate masks IF on
// entry, and the supervisor never enables interrupts in the handler.
unsafe impl<T> Sync for CpuCell<T> {}
static STATE: CpuCell<Option<Runtime>> = CpuCell(UnsafeCell::new(None));

unsafe extern "C" {
    pub(crate) fn zilla_native_timer_irq();
}

// The PIC delivers IRQ0 at 0x20 after reprogramming. An interrupt gate
// clears IF while the handler executes. All 15 GP registers are captured,
// including RAX (which the cooperative INT 0x80 ABI intentionally does not
// preserve). On success we overwrite the 20-qword context and IRETQ to the
// next user thread with IF=1. On completion we jump to the saved kernel
// continuation (which resets CR3 and RSP) with IF already disabled.
global_asm!(r#"
    .section .text
    .global zilla_native_timer_irq
zilla_native_timer_irq:
    push rax
    mov rax, [rsp + 16]
    and rax, 3
    cmp rax, 3
    pop rax
    jne zilla_native_timer_bad_cpl

    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    push rdi
    push rsi
    push rdx
    push rcx
    push r8
    push r9
    push r10
    push r11
    push rax
    mov r12, rsp
    and rsp, -16
    mov rdi, r12
    call zilla_native_timer_dispatch
    mov rsp, r12
    mov r11, rax

    // Acknowledge only master PIC IRQ0. Both RAX and RDX are saved.
    mov dx, 0x20
    mov al, 0x20
    out dx, al
    cmp r11, -1
    je zilla_native_resume

    pop rax
    pop r11
    pop r10
    pop r9
    pop r8
    pop rcx
    pop rdx
    pop rsi
    pop rdi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    iretq

zilla_native_timer_bad_cpl:
    cli
    mov rdi, 0x20
    xor esi, esi
    mov rdx, [rsp]
    xor ecx, ecx
    and rsp, -16
    call zilla_exception_fatal
"#);

#[inline]
unsafe fn outb(port: u16, value: u8) {
    // SAFETY: known legacy IRQ controller/PIT I/O ports, QEMU-only feature.
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value,
             options(nomem, nostack, preserves_flags));
    }
}
#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: access only 8259 PIC mask registers in QEMU.
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value,
             options(nomem, nostack, preserves_flags));
    }
    value
}
#[inline]
unsafe fn wait_io() {
    // SAFETY: legacy port 0x80 is used as a small I/O delay.
    unsafe { outb(0x80, 0) }
}

struct SavedPicMasks { master: u8, slave: u8 }

fn initialize_timer() -> SavedPicMasks {
    // SAFETY: interrupts were disabled since boot; no other logical CPU or
    // legacy PIC client is active in this isolated test ISO.
    unsafe {
        asm!("cli", options(nomem, nostack));
        let master = inb(0x21);
        let slave = inb(0xa1);
        // ICW1: initialize two 8259 PICs, 4 ICWs. Remap IRQ0 to vector 0x20.
        outb(0x20, 0x11); wait_io();
        outb(0xa0, 0x11); wait_io();
        outb(0x21, 0x20); wait_io();
        outb(0xa1, 0x28); wait_io();
        outb(0x21, 0x04); wait_io(); // slave connected to master IRQ2
        outb(0xa1, 0x02); wait_io();
        outb(0x21, 0x01); wait_io(); // 8086 mode
        outb(0xa1, 0x01); wait_io();
        outb(0x21, 0xfe); // mask everything except master IRQ0
        outb(0xa1, 0xff); // mask all slave IRQs
        // PIT channel 0: LSB+MSB, rate-generator mode 2, binary.
        outb(0x43, 0x34);
        outb(0x40, PIT_DIVISOR as u8);
        outb(0x40, (PIT_DIVISOR >> 8) as u8);
        SavedPicMasks { master, slave }
    }
}
fn stop_timer(previous: SavedPicMasks) {
    // SAFETY: we returned from an IRQ through kernel continuation with IF=0.
    // Restore interrupt masks; this feature does not start any other IRQ
    // consumers and QEMU exits immediately after result verification.
    unsafe {
        asm!("cli", options(nomem, nostack));
        outb(0x21, previous.master);
        outb(0xa1, previous.slave);
    }
}

fn frame_valid(frame: &TimerFrame, index: usize) -> bool {
    let (low, high) = if index == 0 {
        (STACK_A - PAGE_SIZE, STACK_A)
    } else {
        (STACK_B - PAGE_SIZE, STACK_B)
    };
    let entry = if index == 0 { CODE } else { ENTRY_B };
    frame.cs == 0x23 && frame.ss == 0x1b
        && frame.rflags & 0x202 == 0x202
        // No IOPL or direction flag in a verified user frame.
        && frame.rflags & (0x3000 | 0x0400) == 0
        // The controlled fixture occupies 17 bytes, not a full code page.
        // Reusing a different task's entry address must fail closed.
        && frame.rip >= entry && frame.rip < entry + 17
        && frame.rsp > low && frame.rsp <= high
}

fn fail(state: &mut Runtime) -> u64 {
    state.failed = true;
    u64::MAX
}

/// IRQ0 dispatcher: return -1 to leave Ring3 or 0 to IRET into selected task.
#[unsafe(no_mangle)]
pub extern "C" fn zilla_native_timer_dispatch(frame: *mut TimerFrame) -> u64 {
    // SAFETY: the interrupt gate disabled IF, and the handler passed a
    // pointer to its own 20-qword kernel trap frame, not user memory.
    let Some(runtime) = (unsafe { &mut *STATE.0.get() }).as_mut() else {
        return u64::MAX;
    };
    if frame.is_null() { return fail(runtime); }
    let current = runtime.current;
    // SAFETY: 15 GP pushes and 5-qword CPU frame; owned TSS.RSP0 stack.
    let saved = unsafe { ptr::read(frame) };
    if !frame_valid(&saved, current) || runtime.ticks >= SAMPLES {
        return fail(runtime);
    }
    // No voluntary yield or syscalls: RBX advances in the user spin loop,
    // while R12 remains a per-thread sentinel across forced switches.
    let counter = saved.gp[GP_RBX];
    if saved.gp[GP_R12] != SENTINELS[current]
        || counter <= runtime.previous_counter[current]
    {
        return fail(runtime);
    }
    runtime.previous_counter[current] = counter;
    runtime.observed[current] = counter;
    runtime.frames[current] = saved;
    runtime.trace[runtime.ticks] = b'A' + current as u8;
    runtime.ticks += 1;

    if runtime.threads.preempt_running(0, runtime.tids[current]).is_err() {
        return fail(runtime);
    }
    if runtime.ticks == SAMPLES {
        if runtime.threads.exit(runtime.tids[0]).is_err()
            || runtime.threads.exit(runtime.tids[1]).is_err()
        {
            return fail(runtime);
        }
        return u64::MAX; // stop both controlled tasks at this timer tick
    }
    let Some(next_tid) = runtime.threads.next_ready(0) else {
        return fail(runtime);
    };
    let next = if next_tid == runtime.tids[0] { 0 }
        else if next_tid == runtime.tids[1] { 1 }
        else { return fail(runtime) };
    if next == current || !frame_valid(&runtime.frames[next], next)
        || runtime.threads.mark_running(0, next_tid).is_err()
    {
        return fail(runtime);
    }
    // SAFETY: the next frame belongs to a separately allocated RW/NX
    // user stack; the same read-only executable page is shared by design.
    unsafe { ptr::write(frame, runtime.frames[next]) };
    runtime.current = next;
    0
}

fn install_fixture(staged: &StagedSpace, hhdm: u64) -> Result<(), Ring3Error> {
    let code = hhdm.checked_add(staged.code_frame)
        .ok_or(Ring3Error::InvalidFixture)? as *mut u8;
    // Two unique register canaries and two independent spin counters.
    let template = [
        0x31, 0xdb,                         // xor ebx, ebx
        0x49, 0xbc, 0, 0, 0, 0, 0, 0, 0, 0, // mov r12, imm64
        0x48, 0xff, 0xc3,                   // spin: inc rbx
        0xeb, 0xfb,                         // jmp -5 (back to inc)
    ];
    let mut first = template;
    let mut second = template;
    first[4..12].copy_from_slice(&SENTINELS[0].to_le_bytes());
    second[4..12].copy_from_slice(&SENTINELS[1].to_le_bytes());
    // SAFETY: exclusive zeroed code frame; both fixed code spans are small,
    // bounded and written through Limine HHDM before Ring 3 execution.
    unsafe {
        ptr::copy_nonoverlapping(first.as_ptr(), code, first.len());
        ptr::copy_nonoverlapping(second.as_ptr(), code.add(128), second.len());
    }
    Ok(())
}

/// # Safety
///
/// Single BSP only, valid Limine/HHDM/staged user pages and immutable code.
/// No APs, external IRQ consumers, or untrusted programs are permitted.
pub unsafe fn run(staged: &StagedSpace, hhdm: u64) -> Result<(), Ring3Error> {
    install_fixture(staged, hhdm)?;
    let mut processes=ProcessTable::new();
    let pid=processes.create("timer-preempt-two", CODE, STACK_A, 1)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    processes.map_region(pid, UserRegion::checked(
        CODE, PAGE_SIZE, Permissions::READ_EXECUTE,
    ).map_err(|_| Ring3Error::InvalidFixture)?)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    processes.map_region(pid, UserRegion::checked(
        STACK_B-PAGE_SIZE, PAGE_SIZE, Permissions::READ_WRITE,
    ).map_err(|_| Ring3Error::InvalidFixture)?)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    processes.prepare(pid).map_err(|_| Ring3Error::InvalidFixture)?;

    let mut threads=ThreadTable::new();
    let a=threads.create(&processes,pid,CODE,STACK_A,1)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    let b=threads.create(&processes,pid,ENTRY_B,STACK_B,1)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    threads.prepare(&processes,a).map_err(|_| Ring3Error::InvalidFixture)?;
    threads.prepare(&processes,b).map_err(|_| Ring3Error::InvalidFixture)?;
    if threads.next_ready(0)!=Some(a)
        || threads.mark_running(0,a).is_err()
        || processes.mark_running(pid).is_err()
    {
        return Err(Ring3Error::InvalidFixture);
    }

    // SAFETY: only BSP enters this opt-in test with maskable interrupts off.
    unsafe {
        *STATE.0.get() = Some(Runtime {
            threads, tids: [a,b],
            frames: [TimerFrame::initial(CODE,STACK_A),
                     TimerFrame::initial(ENTRY_B,STACK_B)],
            current: 0, ticks: 0, trace: [0; SAMPLES],
            previous_counter: [0;2], observed:[0;2], failed:false,
        });
    }
    native_ring3::install_gdt_tss_and_idt()?;
    let masks=initialize_timer();
    serial::write("ZILLAOS_TIMER_PREEMPT_ENTER\n");
    serial::write("ZILLAOS_TIMER_PREEMPT_RATE_HZ=0x0000000000000064\n");
    // SAFETY: IRETQ sets IF in the verified user frame, allowing PIT IRQ0 to
    // preempt code that never invokes yield or exit. The IRQ handler restores
    // the original supervisor continuation with interrupts disabled.
    unsafe { native_ring3::enter_staged(staged.root,CODE,STACK_A) };
    stop_timer(masks);
    serial::write("ZILLAOS_TIMER_PREEMPT_RETURNED\n");

    let state=unsafe { &mut *STATE.0.get() }.take()
        .ok_or(Ring3Error::UnexpectedExit)?;
    if state.failed || state.trace != *EXPECTED || state.ticks!=SAMPLES
        || state.observed[0]==0 || state.observed[1]==0
        || state.threads.get(a).is_none_or(|t|t.state!=ThreadState::Exited)
        || state.threads.get(b).is_none_or(|t|t.state!=ThreadState::Exited)
    {
        serial::write("ZILLAOS_TIMER_PREEMPT_VERIFICATION_FAILED\n");
        return Err(Ring3Error::UnexpectedExit);
    }
    processes.mark_exited(pid,ExitReason::Normal(0))
        .map_err(|_| Ring3Error::UnexpectedExit)?;
    processes.reap(pid).map_err(|_| Ring3Error::UnexpectedExit)?;
    serial::write("ZILLAOS_TIMER_PREEMPT_TRACE=ABABABAB\n");
    serial::write("ZILLAOS_TIMER_PREEMPT_TICKS=0x0000000000000008\n");
    serial::write("ZILLAOS_TIMER_PREEMPT_BOTH_PROGRESS\n");
    serial::write("ZILLAOS_TIMER_PREEMPT_OK\n");
    Ok(())
}
