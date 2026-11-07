//! CPU0-only real timer IRQ0 preemption of two controlled CPL3 contexts.
//! Only compiled into the disposable QEMU `ring3-preempt` ISO.
//! Local xAPIC MMIO timer interrupt-driven switching, not production APIC/SMP
//! scheduling, process isolation across CR3s, or a GUI.
use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::ptr;
use zillaos_kernel::{
    memory::PAGE_SIZE,
    process::{ExitReason, ProcessTable},
    process::address_space::{Permissions, UserRegion},
    process::thread::{ThreadState, ThreadTable, Tid},
};
use crate::{native_ring3::{self, Ring3Error}, native_vm_staging::{StagedSpace, LAPIC_MMIO_VA}, serial};

const CODE: u64 = 0x0040_0000;
const STACK_A: u64 = 0x0080_0000;
const STACK_B: u64 = 0x007f_e000;
const ENTRY_B: u64 = CODE + 128;
const SAMPLES: usize = 8;
const EXPECTED: &[u8; SAMPLES] = b"ABABABAB";
const SENTINELS: [u64; 2] = [0x9abc_0001_55aa_0001, 0x9abc_0002_55aa_0002];
// QEMU emulates the LAPIC timer, but the clock frequency is not calibrated
// here. Tick counts are real interrupts; they are NOT seconds or CPU %.
const LAPIC_INITIAL_COUNT: u64 = 200_000;
const LAPIC_TPR: usize = 0x080;
const LAPIC_EOI: usize = 0x0b0;
const LAPIC_SVR: usize = 0x0f0;
const LAPIC_LVT_TIMER: usize = 0x320;
const LAPIC_INIT_COUNT: usize = 0x380;
const LAPIC_CUR_COUNT: usize = 0x390;
const LAPIC_DIVIDE: usize = 0x3e0;

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

// The local APIC timer delivers vector 0x20. An interrupt gate
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

    // The xAPIC EOI register is supervisor-only, uncacheable MMIO at
    // 0x7fb0b0 in the verified staged PML4. All GP registers are saved.
    mov rax, 0x7fb0b0
    mov dword ptr [rax], 0
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

/// One disposable QEMU test-only xAPIC timer, using a verified
/// supervisor-only UC/NX mapping in the independent staged page tables.
struct SavedTimer {
    previous_tpr: u32,
    previous_svr: u32,
    previous_lvt: u32,
    previous_divide: u32,
}

#[inline]
unsafe fn read_apic(offset: usize) -> u32 {
    // SAFETY: caller must have switched CR3 to the staged root, whose
    // supervisor-only PTE at LAPIC_MMIO_VA was checked by the page walker.
    unsafe { ptr::read_volatile((LAPIC_MMIO_VA as usize + offset) as *const u32) }
}
#[inline]
unsafe fn write_apic(offset: usize, value: u32) {
    // SAFETY: same validated PCD/PWT, NX, U/S=0 PTE as read_apic.
    unsafe { ptr::write_volatile((LAPIC_MMIO_VA as usize + offset) as *mut u32, value) }
}

unsafe fn with_staged_cr3<T>(root: u64, f: impl FnOnce() -> T) -> T {
    let original: u64;
    // SAFETY: the fixture runs on BSP with IF=0. The staged root mirrors
    // all supervisor higher-half mappings needed by the executing kernel,
    // current stack and code, and adds the isolated xAPIC MMIO mapping.
    unsafe {
        asm!("mov {}, cr3", out(reg) original, options(nomem, nostack));
        asm!("mov cr3, {}", in(reg) root, options(nostack));
    }
    let result = f();
    // SAFETY: restore Limine's original CR3 before enabling user IF.
    unsafe { asm!("mov cr3, {}", in(reg) original, options(nostack)) };
    result
}

/// Configure the local APIC timer in periodic mode using MMIO instead of
/// x2APIC MSRs. QEMU TCG can expose xAPIC even when CPUID.x2APIC is absent.
///
/// The count is *not calibrated* to milliseconds, so the test asserts only
/// real interrupt arrivals and forward progress of independent contexts.
fn initialize_timer() -> Result<SavedTimer, Ring3Error> {
    // SAFETY: the x86-64 intrinsic saves RBX; leaf 1 exists on QEMU qemu64.
    let cpu = unsafe { core::arch::x86_64::__cpuid(1) };
    if cpu.edx & (1 << 9) == 0 {
        serial::write("ZILLAOS_TIMER_XAPIC_UNAVAILABLE\n");
        return Err(Ring3Error::InvalidFixture);
    }
    // SAFETY: called with the staged PML4 active and IF=0, after mapping the
    // APIC physical base via the validated supervisor-only MMIO window.
    unsafe {
        asm!("cli", options(nomem, nostack));
        let previous_tpr = read_apic(LAPIC_TPR);
        let previous_svr = read_apic(LAPIC_SVR);
        let previous_lvt = read_apic(LAPIC_LVT_TIMER);
        let previous_divide = read_apic(LAPIC_DIVIDE);
        serial::write("ZILLAOS_TIMER_XAPIC_ENABLED\n");
        write_apic(LAPIC_TPR, 0);
        write_apic(LAPIC_SVR, (previous_svr & !0x1ff) | 0x1ff);
        write_apic(LAPIC_INIT_COUNT, 0);
        write_apic(LAPIC_DIVIDE, 0x03);
        write_apic(LAPIC_LVT_TIMER, (1 << 17) | 0x20);
        write_apic(LAPIC_INIT_COUNT, LAPIC_INITIAL_COUNT as u32);
        let remaining = read_apic(LAPIC_CUR_COUNT);
        serial::write("ZILLAOS_TIMER_LAPIC_INITIAL_COUNT=");
        serial::write_hex(LAPIC_INITIAL_COUNT);
        serial::write("\nZILLAOS_TIMER_LAPIC_CURRENT_COUNT=");
        serial::write_hex(remaining as u64);
        serial::write("\n");
        if remaining == 0 {
            return Err(Ring3Error::InvalidFixture);
        }
        serial::write("ZILLAOS_TIMER_LAPIC_PERIODIC_ARMED\n");
        Ok(SavedTimer { previous_tpr, previous_svr, previous_lvt,
                        previous_divide })
    }
}
fn stop_timer(previous: SavedTimer) {
    // SAFETY: called under the staged root with IF=0 after returning to
    // the original kernel continuation; quiesce the device before exit.
    unsafe {
        asm!("cli", options(nomem, nostack));
        write_apic(LAPIC_LVT_TIMER, (1 << 16) | 0x20);
        write_apic(LAPIC_INIT_COUNT, 0);
        write_apic(LAPIC_DIVIDE, previous.previous_divide);
        write_apic(LAPIC_LVT_TIMER, previous.previous_lvt);
        write_apic(LAPIC_SVR, previous.previous_svr);
        write_apic(LAPIC_TPR, previous.previous_tpr);
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
    if runtime.ticks == 0 {
        serial::write("ZILLAOS_TIMER_FIRST_LAPIC_IRQ_ENTERED\n");
    }
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
    // Account exactly this delivered, verified timer interrupt for the
    // interrupted running TID before changing its Running->Ready state.
    if runtime.threads.charge_timer_tick(0, runtime.tids[current]).is_err() {
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
    // The independent staged root also owns a supervisor-only xAPIC MMIO
    // page. Temporarily use it in Ring0 to arm/disarm the timer; restore
    // the bootloader root before entering the two user contexts.
    let timer_state = unsafe { with_staged_cr3(staged.root, initialize_timer) }?;
    serial::write("ZILLAOS_TIMER_PREEMPT_ENTER\n");
    serial::write("ZILLAOS_TIMER_SOURCE=XAPIC_LOCAL_TIMER\n");
    // SAFETY: use the *timer-enabled* entry; the normal entry forces IF=0
    // and silently prevents all IRQ delivery. The APIC IRQ handler restores
    // the original supervisor continuation with interrupts disabled.
    unsafe { native_ring3::enter_staged_with_timer(staged.root,CODE,STACK_A) };
    unsafe { with_staged_cr3(staged.root, || stop_timer(timer_state)) };
    serial::write("ZILLAOS_TIMER_PREEMPT_RETURNED\n");

    let state=unsafe { &mut *STATE.0.get() }.take()
        .ok_or(Ring3Error::UnexpectedExit)?;
    if state.failed || state.trace != *EXPECTED || state.ticks!=SAMPLES
        || state.observed[0]==0 || state.observed[1]==0
        || state.threads.get(a).is_none_or(|t|t.timer_ticks!=4)
        || state.threads.get(b).is_none_or(|t|t.timer_ticks!=4)
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
    serial::write("ZILLAOS_TIMER_THREAD_A_TICKS=0x0000000000000004\n");
    serial::write("ZILLAOS_TIMER_THREAD_B_TICKS=0x0000000000000004\n");
    serial::write("ZILLAOS_TIMER_PREEMPT_BOTH_PROGRESS\n");
    serial::write("ZILLAOS_TIMER_PREEMPT_OK\n");
    Ok(())
}
