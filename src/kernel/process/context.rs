//! Register metadata for a future x86-64 IRETQ Ring 3 entry.
//! Constructing it does not load CR3, change privilege or run instructions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitialUserContext {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
}

impl InitialUserContext {
    pub const fn new(entry: u64, stack_top: u64) -> Self {
        // Bit 1 is architecturally reserved/set. IF is intentionally off:
        // an interrupt controller, TSS and user fault recovery do not exist yet.
        Self {
            rip: entry,
            rsp: stack_top,
            rflags: 0x2,
        }
    }
}
