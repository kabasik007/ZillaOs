//! Controlled two-thread CPL3 cooperative switching fixture, single CPU0.
//! This is a real trap-frame IRETQ switch with two distinct user stacks,
//! but NOT preemption, SMP, a production process runner, or a general loader.
use crate::{
    native_ring3::{self, Ring3Error},
    native_vm_staging::StagedSpace,
    serial,
};
use core::cell::UnsafeCell;
use core::ptr;
use zillaos_kernel::{
    memory::PAGE_SIZE,
    process::address_space::{Permissions, UserRegion},
    process::syscall::{SYS_EXIT, SYS_YIELD},
    process::thread::{ThreadState, ThreadTable, Tid},
    process::{ExitReason, ProcessTable},
};

const CODE: u64 = 0x0040_0000;
const STACK_A: u64 = 0x0080_0000;
const STACK_B: u64 = 0x007f_e000;
const ENTRY_B: u64 = CODE + 128;
const EXPECTED: &[u8; 6] = b"ABABAB";

/// Exact 14 pushed GP registers + 5-qword CPL3 IRET frame layout.
/// The INT 0x80 ABI treats RAX as a syscall return register (restored as 0);
/// all other saved GP registers are copied across context switches.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserTrapFrame {
    pub gp: [u64; 14],
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl UserTrapFrame {
    const fn initial(entry: u64, stack: u64) -> Self {
        Self {
            gp: [0; 14],
            rip: entry,
            cs: 0x23,
            rflags: 0x2,
            rsp: stack,
            ss: 0x1b,
        }
    }
}

struct Runtime {
    threads: ThreadTable,
    tids: [Tid; 2],
    saved: [UserTrapFrame; 2],
    current: usize,
    finished: [bool; 2],
    yields: [u8; 6],
    yield_count: usize,
    faulted: bool,
}
struct CpuLocal<T>(UnsafeCell<T>);
// SAFETY: single BSP only, interrupts masked, no concurrent readers.
unsafe impl<T> Sync for CpuLocal<T> {}
static STATE: CpuLocal<Option<Runtime>> = CpuLocal(UnsafeCell::new(None));

const CODE_BYTES: [u8; 42] = [
    0xbb, 1, 0, 0, 0, // mov ebx, 1 or 2
    0x49, 0x89, 0xdc, // mov r12, rbx
    0x41, 0xbd, 3, 0, 0, 0, // mov r13d, 3
    0xb8, 3, 0, 0, 0, // .loop: mov eax, SYS_YIELD
    0xcd, 0x80, // int 0x80
    0x4c, 0x39, 0xe3, // cmp rbx, r12; both MUST survive
    0x75, 14, // jne .bad: UD2
    0x49, 0xff, 0xcd, // dec r13
    0x75, 0xef, // jnz .loop
    0xb8, 2, 0, 0, 0, // mov eax, SYS_EXIT
    0x31, 0xff, // xor edi, edi
    0xcd, 0x80, // int 0x80
    0x0f, 0x0b, // .bad: UD2, never reached
];

fn install_fixture(staged: &StagedSpace, hhdm: u64) -> Result<(), Ring3Error> {
    let code = hhdm
        .checked_add(staged.code_frame)
        .ok_or(Ring3Error::InvalidFixture)? as *mut u8;
    let stack_b = staged.stack2_frame;
    let physical = hhdm
        .checked_add(stack_b)
        .ok_or(Ring3Error::InvalidFixture)?;
    if code as u64 % PAGE_SIZE != 0 || physical % PAGE_SIZE != 0 {
        return Err(Ring3Error::InvalidFixture);
    }
    let mut a = CODE_BYTES;
    let mut b = CODE_BYTES;
    a[1] = 1;
    b[1] = 2;
    // SAFETY: both user-code slots are within the exclusive HHDM-mapped
    // code frame. No user execution has begun; page tables map RX.
    unsafe {
        ptr::copy_nonoverlapping(a.as_ptr(), code, a.len());
        ptr::copy_nonoverlapping(b.as_ptr(), code.add(128), b.len());
    }
    Ok(())
}

fn valid_frame(frame: &UserTrapFrame, idx: usize) -> bool {
    let (start, end) = if idx == 0 {
        (STACK_A - PAGE_SIZE, STACK_A)
    } else {
        (STACK_B - PAGE_SIZE, STACK_B)
    };
    frame.cs == 0x23
        && frame.ss == 0x1b
        && frame.rflags & 0x2 != 0
        && frame.rflags & 0x200 == 0
        && frame.rip >= CODE
        && frame.rip < CODE + PAGE_SIZE
        && frame.rsp > start
        && frame.rsp <= end
}

fn fail(state: &mut Runtime) -> u64 {
    state.faulted = true;
    u64::MAX // existing interrupt continuation resumes the kernel
}

/// Executed only on INT 0x80 from verified CPL3.
pub fn dispatch(
    number: u64,
    arg0: u64,
    _arg1: u64,
    _arg2: u64,
    saved_user_frame: *mut UserTrapFrame,
) -> u64 {
    // SAFETY: single CPU0 with interrupts off, pointer is the saved Ring3
    // trap frame on the verified TSS.RSP0 stack (not a user pointer).
    let Some(state) = (unsafe { &mut *STATE.0.get() }).as_mut() else {
        return u64::MAX;
    };
    if saved_user_frame.is_null() {
        return fail(state);
    }
    let current = state.current;
    // SAFETY: trap assembly pushes exactly 19 qwords before dispatch.
    let current_frame = unsafe { ptr::read(saved_user_frame) };
    if !valid_frame(&current_frame, current) || state.finished[current] {
        return fail(state);
    }
    // The syscall ABI deliberately ignores unused argument registers.
    // The fixture permits exactly SYS_YIELD and SYS_EXIT(0), with no
    // arbitrary pointers, I/O handles or user memory copied into kernel.
    match number {
        SYS_YIELD => {
            // No user pointers or handles are accepted by SYS_YIELD.
            if state.yield_count >= EXPECTED.len() {
                return fail(state);
            }
            if state.threads.yield_running(state.tids[current]).is_err() {
                return fail(state);
            }
            state.saved[current] = current_frame;
            state.yields[state.yield_count] = b'A' + current as u8;
            state.yield_count += 1;
        }
        SYS_EXIT => {
            if arg0 != 0 {
                return fail(state);
            }
            if state.yield_count != EXPECTED.len()
                || state.threads.exit(state.tids[current]).is_err()
            {
                return fail(state);
            }
            state.finished[current] = true;
            serial::write(if current == 0 {
                "ZILLAOS_NATIVE_THREAD_A_EXIT\n"
            } else {
                "ZILLAOS_NATIVE_THREAD_B_EXIT\n"
            });
        }
        _ => return fail(state),
    }
    if state.finished.iter().all(|done| *done) {
        return u64::MAX; // return to the original kernel stack/CR3
    }
    // Use the exact same CPU0 Round Robin selector used by ThreadTable host
    // tests. No arbitrary user RIP or RSP is allowed through the boundary.
    let Some(next_tid) = state.threads.next_ready(0) else {
        return fail(state);
    };
    let next = if next_tid == state.tids[0] {
        0
    } else if next_tid == state.tids[1] {
        1
    } else {
        return fail(state);
    };
    if next == current
        || state.finished[next]
        || state.threads.mark_running(0, next_tid).is_err()
        || !valid_frame(&state.saved[next], next)
    {
        return fail(state);
    }
    // SAFETY: the CPL3 IRET frame belongs to the same saved CPU0
    // interrupt. Overwrite ALL registers and RIP/RSP to resume the next
    // thread rather than returning into the previous thread's user stack.
    unsafe { ptr::write(saved_user_frame, state.saved[next]) };
    state.current = next;
    0 // INT 0x80 returns 0 to the resumed user thread in RAX
}

/// # Safety
///
/// Requires staged user PML4 and unshared owned pages, no SMP or timer IRQ.
pub unsafe fn run(staged: &StagedSpace, hhdm: u64) -> Result<(), Ring3Error> {
    install_fixture(staged, hhdm)?;

    let mut processes = ProcessTable::new();
    let pid = processes
        .create("ring3-cooperative-threads", CODE, STACK_A, 1)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    processes
        .map_region(
            pid,
            UserRegion::checked(CODE, PAGE_SIZE, Permissions::READ_EXECUTE)
                .map_err(|_| Ring3Error::InvalidFixture)?,
        )
        .map_err(|_| Ring3Error::InvalidFixture)?;
    processes
        .map_region(
            pid,
            UserRegion::checked(STACK_B - PAGE_SIZE, PAGE_SIZE, Permissions::READ_WRITE)
                .map_err(|_| Ring3Error::InvalidFixture)?,
        )
        .map_err(|_| Ring3Error::InvalidFixture)?;
    processes
        .prepare(pid)
        .map_err(|_| Ring3Error::InvalidFixture)?;

    let mut threads = ThreadTable::new();
    let a = threads
        .create(&processes, pid, CODE, STACK_A, 1)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    let b = threads
        .create(&processes, pid, ENTRY_B, STACK_B, 1)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    threads
        .prepare(&processes, a)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    threads
        .prepare(&processes, b)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    if threads.next_ready(0) != Some(a) {
        return Err(Ring3Error::InvalidFixture);
    }
    threads
        .mark_running(0, a)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    processes
        .mark_running(pid)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    // SAFETY: static BSP-only execution; no dynamic buffers, no preemption.
    unsafe {
        *STATE.0.get() = Some(Runtime {
            threads,
            tids: [a, b],
            saved: [
                UserTrapFrame::initial(CODE, STACK_A),
                UserTrapFrame::initial(ENTRY_B, STACK_B),
            ],
            current: 0,
            finished: [false, false],
            yields: [0; 6],
            yield_count: 0,
            faulted: false,
        });
    }
    native_ring3::install_gdt_tss_and_idt()?;
    serial::write("ZILLAOS_NATIVE_THREADS_ENTER\n");
    // SAFETY: controlled executable fixture, CPU0 only, valid GDT/TSS/IDT.
    unsafe { native_ring3::enter_staged(staged.root, CODE, STACK_A) };
    serial::write("ZILLAOS_NATIVE_THREADS_RETURNED\n");

    let runtime = unsafe { &mut *STATE.0.get() }
        .take()
        .ok_or(Ring3Error::UnexpectedExit)?;
    if runtime.faulted
        || runtime.yield_count != EXPECTED.len()
        || runtime.yields != *EXPECTED
        || !runtime.finished.iter().all(|done| *done)
        || runtime
            .threads
            .get(a)
            .is_none_or(|t| t.state != ThreadState::Exited)
        || runtime
            .threads
            .get(b)
            .is_none_or(|t| t.state != ThreadState::Exited)
    {
        return Err(Ring3Error::UnexpectedExit);
    }
    let mut proc_state = processes;
    proc_state
        .mark_exited(pid, ExitReason::Normal(0))
        .map_err(|_| Ring3Error::UnexpectedExit)?;
    proc_state
        .reap(pid)
        .map_err(|_| Ring3Error::UnexpectedExit)?;

    serial::write("ZILLAOS_NATIVE_THREAD_TRACE=ABABAB\n");
    serial::write("ZILLAOS_NATIVE_THREADS_BOTH_EXITED\n");
    Ok(())
}
