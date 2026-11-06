//! Opt-in one-shot Ring 3 controlled native test. Not an ELF/PE loader,
//! preemptive scheduler or public execution service. The normal desktop never
//! installs these descriptors or switches CR3.
use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::mem::size_of;
use core::ptr;
use zillaos_kernel::memory::PAGE_SIZE;
use zillaos_kernel::process::{
    ExitReason, Process, ProcessTable, UserFault,
    address_space::{Permissions, UserRegion},
    syscall::{self, Action, Request},
};

use crate::{native_vm_staging::StagedSpace, serial};

const KERNEL_CODE: u16 = 0x08;
#[cfg(all(feature = "native-ring3-guard-fault", feature = "native-ring3-kernel-fault"))]
compile_error!("Choose only one native Ring3 negative-fault fixture");
#[cfg(all(feature = "native-ring3-threads", any(feature = "native-ring3-guard-fault", feature = "native-ring3-kernel-fault")))]
compile_error!("Cooperative threads fixture is incompatible with Ring3 fault variants");

const CODE_VA: u64 = 0x400000;
const DATA_VA: u64 = 0x401000;
#[cfg(feature = "native-ring3-guard-fault")]
const GUARD_VA: u64 = 0x7fe000;
const STACK_TOP: u64 = 0x800000;
const RSP0_STACK_BYTES: usize = 16 * 1024;
const FAULT_IST_BYTES: usize = 8 * 1024;

const HELLO: &[u8] = b"Hello from ZillaOS Native Ring3!\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ring3Error {
    InvalidNx,
    InvalidDescriptorState,
    InvalidFixture,
    UnexpectedExit,
    ExitCode(i32),
    Fault(UserFault),
}

#[repr(C, packed)]
struct Tss {
    reserved0: u32,
    rsp: [u64; 3],
    reserved1: u64,
    ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    io_map_base: u16,
}

impl Tss {
    const ZERO: Self = Self {
        reserved0: 0, rsp: [0; 3], reserved1: 0, ist: [0; 7],
        reserved2: 0, reserved3: 0, io_map_base: size_of::<Tss>() as u16,
    };
}

#[repr(C, packed)]
struct DescriptorPointer {
    limit: u16,
    base: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IdtGate {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}
impl IdtGate {
    const ABSENT: Self = Self {
        offset_low: 0, selector: 0, ist: 0, attributes: 0,
        offset_mid: 0, offset_high: 0, reserved: 0,
    };
    fn new(handler: u64, dpl3: bool, ist: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector: KERNEL_CODE,
            ist,
            attributes: if dpl3 { 0xee } else { 0x8e },
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(align(16))]
struct Gdt([u64; 7]);
#[repr(align(16))]
struct Idt([IdtGate; 256]);
#[repr(align(16))]
struct Stack<const N: usize>([u8; N]);

struct SingleCpuCell<T>(UnsafeCell<T>);
// SAFETY: Only the BSP executes this feature, with maskable interrupts off.
unsafe impl<T> Sync for SingleCpuCell<T> {}

static GDT: SingleCpuCell<Gdt> = SingleCpuCell(UnsafeCell::new(Gdt([0; 7])));
static TSS: SingleCpuCell<Tss> = SingleCpuCell(UnsafeCell::new(Tss::ZERO));
static IDT: SingleCpuCell<Idt> = SingleCpuCell(UnsafeCell::new(Idt([IdtGate::ABSENT; 256])));
static RSP0_STACK: SingleCpuCell<Stack<RSP0_STACK_BYTES>> =
    SingleCpuCell(UnsafeCell::new(Stack([0; RSP0_STACK_BYTES])));
static FAULT_STACK: SingleCpuCell<Stack<FAULT_IST_BYTES>> =
    SingleCpuCell(UnsafeCell::new(Stack([0; FAULT_IST_BYTES])));

#[unsafe(no_mangle)]
static mut ZILLA_NATIVE_RESUME_RSP: u64 = 0;
#[unsafe(no_mangle)]
static mut ZILLA_NATIVE_RESUME_CR3: u64 = 0;

struct RunState {
    process: Process,
    exit_reason: Option<ExitReason>,
    written: u64,
}
static RUN_STATE: SingleCpuCell<Option<RunState>> = SingleCpuCell(UnsafeCell::new(None));

unsafe extern "C" {
    fn zilla_native_enter(new_cr3: u64, entry: u64, user_rsp: u64);
    fn zilla_native_interrupt();
    fn zilla_native_fault_ud();
    fn zilla_native_fault_gp();
    fn zilla_native_fault_pf();
    fn zilla_native_fault_df();
}

global_asm!(r#"
    .section .text
    .global zilla_native_enter
zilla_native_enter:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    mov qword ptr [rip + ZILLA_NATIVE_RESUME_RSP], rsp
    mov rax, cr3
    mov qword ptr [rip + ZILLA_NATIVE_RESUME_CR3], rax
    mov cr3, rdi
    push 0x1b
    push rdx
    push 0x2
    push 0x23
    push rsi
    iretq

    .global zilla_native_resume
zilla_native_resume:
    mov rax, qword ptr [rip + ZILLA_NATIVE_RESUME_CR3]
    mov cr3, rax
    mov rsp, qword ptr [rip + ZILLA_NATIVE_RESUME_RSP]
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

    .global zilla_native_interrupt
zilla_native_interrupt:
    cli
    push r11
    mov r11, [rsp + 16]
    and r11, 3
    cmp r11, 3
    pop r11
    jne zilla_native_bad_trap
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
    mov r12, rsp
    and rsp, -16
    mov rcx, rdx
    mov rdx, rsi
    mov rsi, rdi
    mov rdi, rax
    mov r8, r12
    call zilla_native_syscall_dispatch
    mov rsp, r12
    cmp rax, -1
    je zilla_native_resume
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

zilla_native_bad_trap:
    mov rdi, 0x80
    xor esi, esi
    mov rdx, [rsp]
    xor ecx, ecx
    and rsp, -16
    call zilla_exception_fatal

    .global zilla_native_fault_ud
zilla_native_fault_ud:
    cli
    mov rax, [rsp + 8]
    and rax, 3
    cmp rax, 3
    jne zilla_exception_ud
    mov rdi, 6
    xor esi, esi
    mov rdx, [rsp]
    xor ecx, ecx
    and rsp, -16
    call zilla_native_fault_dispatch
    jmp zilla_native_resume

    .global zilla_native_fault_gp
zilla_native_fault_gp:
    cli
    mov rax, [rsp + 16]
    and rax, 3
    cmp rax, 3
    jne zilla_exception_gp
    mov rdi, 13
    mov rsi, [rsp]
    mov rdx, [rsp + 8]
    xor ecx, ecx
    and rsp, -16
    call zilla_native_fault_dispatch
    jmp zilla_native_resume

    .global zilla_native_fault_pf
zilla_native_fault_pf:
    cli
    mov rax, [rsp + 16]
    and rax, 3
    cmp rax, 3
    jne zilla_exception_pf
    mov rdi, 14
    mov rsi, [rsp]
    mov rdx, [rsp + 8]
    mov rcx, cr2
    and rsp, -16
    call zilla_native_fault_dispatch
    jmp zilla_native_resume

    .global zilla_native_fault_df
zilla_native_fault_df:
    cli
    mov rdi, 8
    mov rsi, [rsp]
    mov rdx, [rsp + 8]
    xor ecx, ecx
    and rsp, -16
    call zilla_exception_fatal
"#);

pub(crate) fn install_gdt_tss_and_idt() -> Result<(), Ring3Error> {
    let gdt = GDT.0.get();
    let tss = TSS.0.get();
    let idt = IDT.0.get();
    let rsp0 = unsafe { ptr::addr_of!((*RSP0_STACK.0.get()).0) as u64 }
        + RSP0_STACK_BYTES as u64;
    let ist1 = unsafe { ptr::addr_of!((*FAULT_STACK.0.get()).0) as u64 }
        + FAULT_IST_BYTES as u64;
    let tss_addr = tss as u64;
    if rsp0 % 16 != 0 || ist1 % 16 != 0 || size_of::<Tss>() != 104 {
        return Err(Ring3Error::InvalidDescriptorState);
    }

    // SAFETY: exclusively accessed by this feature before entering userspace.
    unsafe {
        ptr::write_unaligned(ptr::addr_of_mut!((*tss).rsp) as *mut u64, rsp0);
        ptr::write_unaligned(ptr::addr_of_mut!((*tss).ist) as *mut u64, ist1);
        let g = &mut (*gdt).0;
        g[0] = 0;
        g[1] = 0x00af_9a00_0000_ffff; // ring0 64-bit code
        g[2] = 0x00cf_9200_0000_ffff; // ring0 data
        g[3] = 0x00cf_f200_0000_ffff; // ring3 data
        g[4] = 0x00af_fa00_0000_ffff; // ring3 64-bit code
        let limit = (size_of::<Tss>() - 1) as u64;
        g[5] = limit
            | ((tss_addr & 0xffff) << 16)
            | (((tss_addr >> 16) & 0xff) << 32)
            | (0x89_u64 << 40)
            | (((tss_addr >> 24) & 0xff) << 56);
        g[6] = tss_addr >> 32;

        let gates = &mut (*idt).0;
        gates[6] = IdtGate::new(zilla_native_fault_ud as *const () as u64, false, 0);
        gates[8] = IdtGate::new(zilla_native_fault_df as *const () as u64, false, 1);
        gates[13] = IdtGate::new(zilla_native_fault_gp as *const () as u64, false, 0);
        gates[14] = IdtGate::new(zilla_native_fault_pf as *const () as u64, false, 0);
        gates[0x80] = IdtGate::new(zilla_native_interrupt as *const () as u64, true, 0);
    }

    let gdtr = DescriptorPointer {
        limit: (size_of::<Gdt>() - 1) as u16,
        base: gdt as u64,
    };
    let idtr = DescriptorPointer {
        limit: (size_of::<Idt>() - 1) as u16,
        base: idt as u64,
    };
    // SAFETY: valid kernel-address descriptors; no interrupts or additional
    // CPUs. Reload CS/SS/DS before enabling the task register and new IDT.
    unsafe {
        asm!(
            "lgdt [{gdtr}]",
            "push 0x08",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, 0x10",
            "mov ss, ax",
            "mov ds, ax",
            "mov es, ax",
            "xor eax, eax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ax, 0x28",
            "ltr ax",
            "lidt [{idtr}]",
            gdtr = in(reg) &gdtr,
            idtr = in(reg) &idtr,
            out("rax") _,
        );
    }
    Ok(())
}

/// # Safety
/// Only invoke after installing GDT/TSS/IDT on the single BSP.
pub(crate) unsafe fn enter_staged(root: u64, entry: u64, stack: u64) {
    // SAFETY: caller guarantees a validated user PML4 and Ring3 context.
    unsafe { zilla_native_enter(root, entry, stack) }
}

fn copy_fixture(staged: &StagedSpace, hhdm: u64) -> Result<(), Ring3Error> {
    // x86_64: push/pop; write(1, 0x401000, n); exit(0); UD2.
    // Intentionally no libraries, ELF/PE loader or filesystem.
    let mut program: [u8; 40] = [
        0x50, 0x58,                   // push rax; pop rax (exercise user stack)
        0xb8, 1, 0, 0, 0,             // mov eax, SYS_WRITE
        0xbf, 1, 0, 0, 0,             // mov edi, STDOUT
        0x48, 0xbe, 0, 0x10, 0x40, 0, 0, 0, 0, 0, // mov rsi, DATA_VA
        0xba, 0, 0, 0, 0,             // mov edx, HELLO.len()
        0xcd, 0x80,                  // int 0x80
        0xb8, 2, 0, 0, 0,             // mov eax, SYS_EXIT
        0x31, 0xff,                  // xor edi, edi
        0xcd, 0x80,                  // int 0x80
        0x0f, 0x0b,                  // UD2 (never reached for success)
    ];
    if program.len() != 40 || HELLO.len() >= 128 {
        return Err(Ring3Error::InvalidFixture);
    }
    program[23..27].copy_from_slice(&(HELLO.len() as u32).to_le_bytes());

    // Negative isolation fixture: after writing from Ring 3, read the
    // *unmapped* stack guard page. The CPU must raise user #PF; the kernel
    // fault handler must return control without executing the trailing UD2.
    #[cfg(feature = "native-ring3-guard-fault")]
    program[29..40].copy_from_slice(&[
        0xbe, 0x00, 0xe0, 0x7f, 0x00, // mov esi, 0x7fe000
        0x8a, 0x06,                   // mov al, byte ptr [rsi] -> user #PF
        0x0f, 0x0b,                   // ud2: should never be reached
        0x90, 0x90,                   // padding
    ]);

    // Negative privilege-boundary fixture. Mov rsi, sign-extended imm32
    // targets the actual, already verified higher-half kernel _start page.
    // A CPL3 read must fault because the new PML4 entry has U/S cleared.
    #[cfg(feature = "native-ring3-kernel-fault")]
    {
        let kernel_target = crate::_start as usize as u64;
        if kernel_target >> 32 != 0xffff_ffff || (kernel_target as u32) < 0x8000_0000 {
            return Err(Ring3Error::InvalidFixture);
        }
        program[29..40].copy_from_slice(&[
            0x48, 0xc7, 0xc6, 0, 0, 0, 0, // mov rsi, sign-extended imm32
            0x8a, 0x06,                   // mov al, byte ptr [rsi]
            0x0f, 0x0b,                   // ud2 if unexpectedly permitted
        ]);
        program[32..36].copy_from_slice(&(kernel_target as u32).to_le_bytes());
    }

    let code_ptr = hhdm.checked_add(staged.code_frame).ok_or(Ring3Error::InvalidFixture)?;
    let data_ptr = hhdm.checked_add(staged.data_frame).ok_or(Ring3Error::InvalidFixture)?;
    // SAFETY: both frames are exclusively owned USABLE frames, initially
    // zeroed in stage(), and are mapped via HHDM. Written only before switch.
    unsafe {
        ptr::copy_nonoverlapping(program.as_ptr(), code_ptr as *mut u8, program.len());
        ptr::copy_nonoverlapping(HELLO.as_ptr(), data_ptr as *mut u8, HELLO.len());
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn zilla_native_syscall_dispatch(
    number: u64, arg0: u64, arg1: u64, arg2: u64,
    _frame: *mut u64,
) -> u64 {
    #[cfg(feature = "native-ring3-threads")]
    {
        return crate::native_ring3_threads::dispatch(
            number, arg0, arg1, arg2,
            _frame.cast::<crate::native_ring3_threads::UserTrapFrame>(),
        );
    }
    #[cfg(not(feature = "native-ring3-threads"))]
    {
    // SAFETY: no concurrent CPUs or interrupts; only one process active.
    let runtime = unsafe { &mut *RUN_STATE.0.get() };
    let Some(state) = runtime.as_mut() else { return u64::MAX };
    let request = Request { number, arg0, arg1, arg2 };
    match syscall::validate(&state.process, request) {
        Ok(Action::Write { address, len }) => {
            let Some(end) = address.checked_add(len) else { return u64::MAX - 1 };
            // In this one-shot fixture, copy only from the data page known
            // to be mapped and validated in stage(). No general copyin yet.
            if address < DATA_VA || end > DATA_VA + PAGE_SIZE || len > 127 {
                return u64::MAX - 1;
            }
            let mut buffer = [0u8; 128];
            for (index, item) in buffer.iter_mut().take(len as usize).enumerate() {
                // SAFETY: checked single-page user data range, no page-table
                // mutations and no other CPU. Reads are bounded byte-wise.
                *item = unsafe { ptr::read_volatile((address + index as u64) as *const u8) };
            }
            let Ok(s) = core::str::from_utf8(&buffer[..len as usize]) else {
                return u64::MAX - 1;
            };
            serial::write("ZILLAOS_NATIVE_STDOUT=");
            serial::write(s);
            state.written = len;
            len
        }
        Ok(Action::Exit { code }) => {
            state.exit_reason = Some(ExitReason::Normal(code));
            u64::MAX // assembly returns to saved kernel continuation
        }
        Ok(Action::Yield) => u64::MAX - 1, // no scheduler in other modes
        Err(_) => u64::MAX - 1,
    }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn zilla_native_fault_dispatch(
    vector: u64, error: u64, rip: u64, cr2: u64,
) {
    serial::write("ZILLAOS_NATIVE_USER_FAULT_VECTOR=");
    serial::write_hex(vector);
    serial::write("\nZILLAOS_NATIVE_USER_FAULT_RIP=");
    serial::write_hex(rip);
    serial::write("\n");
    let fault = match vector {
        6 => UserFault::InvalidOpcode,
        13 => UserFault::GeneralProtection,
        14 => UserFault::PageFault { address: cr2, error },
        x => UserFault::Other(x as u8),
    };
    // SAFETY: one-shot BSP-only test; same lifecycle as syscall handler.
    if let Some(state) = unsafe { &mut *RUN_STATE.0.get() }.as_mut() {
        state.exit_reason = Some(ExitReason::Fault(fault));
    }
}

/// # Safety
///
/// Call only on a single-core test ISO after stage() succeeded, with no
/// asynchronous interrupts, valid Limine HHDM and a new supervisor-mirrored
/// PML4. Does not accept user-controlled code or files.
pub unsafe fn run(staged: &StagedSpace, hhdm: u64) -> Result<(), Ring3Error> {
    // SAFETY: read EFER MSR, which is supported on x86-64.
    let lo: u32;
    let hi: u32;
    unsafe { asm!("rdmsr", in("ecx") 0xc000_0080_u32, out("eax") lo, out("edx") hi) };
    let efer = (u64::from(hi) << 32) | u64::from(lo);
    if efer & (1 << 11) == 0 { return Err(Ring3Error::InvalidNx) }

    #[cfg(feature = "native-ring3-threads")]
    {
        return unsafe { crate::native_ring3_threads::run(staged, hhdm) };
    }
    #[cfg(not(feature = "native-ring3-threads"))]
    {
    let mut table = ProcessTable::new();
    let pid = table.create("native-ring3-fixture", CODE_VA, STACK_TOP, 1)
        .map_err(|_| Ring3Error::InvalidFixture)?;
    for (base, permissions) in [
        (CODE_VA, Permissions::READ_EXECUTE),
        (DATA_VA, Permissions::READ_WRITE),
    ] {
        table.map_region(pid, UserRegion::checked(base, PAGE_SIZE, permissions)
            .map_err(|_| Ring3Error::InvalidFixture)?)
            .map_err(|_| Ring3Error::InvalidFixture)?;
    }
    table.prepare(pid).map_err(|_| Ring3Error::InvalidFixture)?;
    copy_fixture(staged, hhdm)?;
    let process = table.process(pid).ok_or(Ring3Error::InvalidFixture)?.clone();
    table.mark_running(pid).map_err(|_| Ring3Error::InvalidFixture)?;
    // SAFETY: exclusive BSP test process, interrupts disabled; stack and
    // translation tables remain live until the process returns.
    unsafe {
        *RUN_STATE.0.get() = Some(RunState { process, exit_reason: None, written: 0 });
    }
    install_gdt_tss_and_idt()?;
    serial::write("ZILLAOS_NATIVE_RING3_ENTER\n");
    // SAFETY: controlled text RX and stack RW user pages; IDT+TSS installed.
    unsafe { zilla_native_enter(staged.root, CODE_VA, STACK_TOP) };
    serial::write("ZILLAOS_NATIVE_RING3_RETURNED\n");
    let state = unsafe { &mut *RUN_STATE.0.get() }.take().ok_or(Ring3Error::UnexpectedExit)?;
    let reason = state.exit_reason.ok_or(Ring3Error::UnexpectedExit)?;
    table.mark_exited(pid, reason).map_err(|_| Ring3Error::UnexpectedExit)?;
    table.reap(pid).map_err(|_| Ring3Error::UnexpectedExit)?;
    match reason {
        #[cfg(feature = "native-ring3-kernel-fault")]
        ExitReason::Fault(UserFault::PageFault { address, error })
            if address == crate::_start as usize as u64
                && error & 0x7 == 0x5
                && state.written == HELLO.len() as u64 =>
        {
            serial::write("ZILLAOS_NATIVE_USER_KERNEL_ACCESS_DENIED\n");
            Ok(())
        }
        #[cfg(feature = "native-ring3-guard-fault")]
        ExitReason::Fault(UserFault::PageFault { address, error })
            if address == GUARD_VA && error & 0x7 == 0x4
                && state.written == HELLO.len() as u64 =>
        {
            serial::write("ZILLAOS_NATIVE_USER_GUARD_FAULT_OK\n");
            Ok(())
        }
        #[cfg(not(any(feature = "native-ring3-guard-fault", feature = "native-ring3-kernel-fault")))]
        ExitReason::Normal(0) if state.written == HELLO.len() as u64 => {
            serial::write("ZILLAOS_NATIVE_EXIT_CODE=0x0000000000000000\n");
            Ok(())
        }
        ExitReason::Normal(code) => Err(Ring3Error::ExitCode(code)),
        ExitReason::Fault(fault) => Err(Ring3Error::Fault(fault)),
        _ => Err(Ring3Error::UnexpectedExit),
    }
    }
}
