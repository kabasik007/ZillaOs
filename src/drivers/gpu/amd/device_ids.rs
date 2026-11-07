//! Diagnostic ASIC hints only. PCI IDs do not establish a board SKU,
//! firmware compatibility, verified IP revisions, or driver operability.
#![forbid(unsafe_code)]

use super::pci::PciIdentity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsicFamily {
    Unknown,
    Polaris10,
    Navi10,
    Navi21,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticId {
    pub device_id: u16,
    pub family: AsicFamily,
}

/// Family-level matches sourced from public PCI ID evidence. A different
/// subsystem/revision may still require a different board policy. No entry
/// grants access to an AMD device.
pub const DIAGNOSTIC_IDS: &[DiagnosticId] = &[
    DiagnosticId {
        device_id: 0x67df,
        family: AsicFamily::Polaris10,
    },
    DiagnosticId {
        device_id: 0x731f,
        family: AsicFamily::Navi10,
    },
    DiagnosticId {
        device_id: 0x73bf,
        family: AsicFamily::Navi21,
    },
];

/// Strict enablement evidence gate reserved for later physical validation.
/// Deliberately empty: there are ZERO AMD boards currently supported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupportedBoard {
    pub device_id: u16,
    pub revision: u8,
    pub subsystem_vendor_id: u16,
    pub subsystem_device_id: u16,
    pub family: AsicFamily,
}

pub const SUPPORTED_BOARDS: &[SupportedBoard] = &[];

pub fn diagnostic_family(device_id: u16) -> AsicFamily {
    DIAGNOSTIC_IDS
        .iter()
        .find(|entry| entry.device_id == device_id)
        .map_or(AsicFamily::Unknown, |entry| entry.family)
}

/// Never infers support from vendor, class, family, or marketing name.
/// Future nonempty entries need exact board and IP/firmware/hardware proof.
pub fn explicitly_supported(id: &PciIdentity) -> bool {
    SUPPORTED_BOARDS.iter().any(|board| {
        board.device_id == id.device_id
            && board.revision == id.revision
            && Some(board.subsystem_vendor_id) == id.subsystem_vendor_id
            && Some(board.subsystem_device_id) == id.subsystem_device_id
            && board.family == diagnostic_family(id.device_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_identity_does_not_grant_support() {
        assert_eq!(diagnostic_family(0x67df), AsicFamily::Polaris10);
        assert_eq!(diagnostic_family(0x731f), AsicFamily::Navi10);
        assert_eq!(diagnostic_family(0x73bf), AsicFamily::Navi21);
        assert_eq!(diagnostic_family(0xffff), AsicFamily::Unknown);
        assert!(SUPPORTED_BOARDS.is_empty());
    }
}
