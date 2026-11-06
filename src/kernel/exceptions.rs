//! Host-testable x86-64 exception metadata. No hardware interaction here.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exception {
    InvalidOpcode,
    GeneralProtection,
    PageFault,
    Other(u8),
}

pub const fn classify(vector: u8) -> Exception {
    match vector {
        6 => Exception::InvalidOpcode,
        13 => Exception::GeneralProtection,
        14 => Exception::PageFault,
        n => Exception::Other(n),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageFaultFlags {
    pub protection_violation: bool,
    pub write: bool,
    pub user: bool,
    pub reserved_bit: bool,
    pub instruction_fetch: bool,
    pub protection_key: bool,
    pub shadow_stack: bool,
    pub sgx: bool,
}

pub const fn page_fault_flags(error_code: u64) -> PageFaultFlags {
    PageFaultFlags {
        protection_violation: (error_code & (1 << 0)) != 0,
        write: (error_code & (1 << 1)) != 0,
        user: (error_code & (1 << 2)) != 0,
        reserved_bit: (error_code & (1 << 3)) != 0,
        instruction_fetch: (error_code & (1 << 4)) != 0,
        protection_key: (error_code & (1 << 5)) != 0,
        shadow_stack: (error_code & (1 << 6)) != 0,
        sgx: (error_code & (1 << 15)) != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_early_kernel_vectors() {
        assert_eq!(classify(6), Exception::InvalidOpcode);
        assert_eq!(classify(13), Exception::GeneralProtection);
        assert_eq!(classify(14), Exception::PageFault);
        assert_eq!(classify(3), Exception::Other(3));
    }

    #[test]
    fn zero_error_code_is_non_present_read_from_supervisor() {
        let f = page_fault_flags(0);
        assert!(!f.protection_violation);
        assert!(!f.write);
        assert!(!f.user);
        assert!(!f.instruction_fetch);
    }

    #[test]
    fn decodes_write_from_user_with_reserved_page_bit() {
        let f = page_fault_flags((1 << 0) | (1 << 1) | (1 << 2) | (1 << 3));
        assert!(f.protection_violation);
        assert!(f.write);
        assert!(f.user);
        assert!(f.reserved_bit);
        assert!(!f.instruction_fetch);
    }

    #[test]
    fn decodes_extended_fault_bits_without_truncation() {
        let f = page_fault_flags((1 << 4) | (1 << 5) | (1 << 6) | (1 << 15));
        assert!(f.instruction_fetch);
        assert!(f.protection_key);
        assert!(f.shadow_stack);
        assert!(f.sgx);
    }
}
