//! A1: hardware-agnostic IP version contracts. No register addresses,
//! MMIO access, microcode, queues, engine initialization or interrupts.
#![forbid(unsafe_code)]

use super::device_ids::AsicFamily;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpKind {
    DisplayDce,
    DisplayDcn,
    Gmc,
    Psp,
    Smu,
    Ih,
    Sdma,
    Gfx,
}

pub const IP_KINDS: [IpKind; 8] = [
    IpKind::DisplayDce,
    IpKind::DisplayDcn,
    IpKind::Gmc,
    IpKind::Psp,
    IpKind::Smu,
    IpKind::Ih,
    IpKind::Sdma,
    IpKind::Gfx,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpVersion {
    pub major: u8,
    pub minor: u8,
    pub revision: u8,
}

/// None means unverified, not missing. Optional blocks may be absent only
/// when real ASIC-specific evidence supports that conclusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpDiscovery {
    Unknown,
    AbsentVerified,
    PresentVerified(IpVersion),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpBlock {
    pub kind: IpKind,
    pub discovery: IpDiscovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsicIpInventory {
    pub family_hint: AsicFamily,
    pub blocks: [IpBlock; 8],
}

impl AsicIpInventory {
    pub const fn unknown(family_hint: AsicFamily) -> Self {
        Self {
            family_hint,
            blocks: [
                IpBlock {
                    kind: IpKind::DisplayDce,
                    discovery: IpDiscovery::Unknown,
                },
                IpBlock {
                    kind: IpKind::DisplayDcn,
                    discovery: IpDiscovery::Unknown,
                },
                IpBlock {
                    kind: IpKind::Gmc,
                    discovery: IpDiscovery::Unknown,
                },
                IpBlock {
                    kind: IpKind::Psp,
                    discovery: IpDiscovery::Unknown,
                },
                IpBlock {
                    kind: IpKind::Smu,
                    discovery: IpDiscovery::Unknown,
                },
                IpBlock {
                    kind: IpKind::Ih,
                    discovery: IpDiscovery::Unknown,
                },
                IpBlock {
                    kind: IpKind::Sdma,
                    discovery: IpDiscovery::Unknown,
                },
                IpBlock {
                    kind: IpKind::Gfx,
                    discovery: IpDiscovery::Unknown,
                },
            ],
        }
    }

    /// A0/A1 never treats inferred/unknown IP revisions as usable.
    pub fn all_blocks_verified(&self) -> bool {
        self.blocks
            .iter()
            .all(|b| b.discovery != IpDiscovery::Unknown)
    }

    pub fn has_verified_display_engine(&self) -> bool {
        let dce = matches!(self.blocks[0].discovery, IpDiscovery::PresentVerified(_));
        let dcn = matches!(self.blocks[1].discovery, IpDiscovery::PresentVerified(_));
        dce ^ dcn
    }
}


/// Explicit per-IP diagnostic contracts. These are metadata only: no register
/// addresses, firmware parser, queue or display scanout is available at A1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayIpContract {
    pub dce: IpDiscovery,
    pub dcn: IpDiscovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GmcContract {
    pub version: IpDiscovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PspContract {
    pub version: IpDiscovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmuContract {
    pub version: IpDiscovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IhContract {
    pub version: IpDiscovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdmaContract {
    pub version: IpDiscovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GfxContract {
    pub version: IpDiscovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedIpContracts {
    pub family_hint: AsicFamily,
    pub display: DisplayIpContract,
    pub gmc: GmcContract,
    pub psp: PspContract,
    pub smu: SmuContract,
    pub ih: IhContract,
    pub sdma: SdmaContract,
    pub gfx: GfxContract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpContractError {
    IncorrectBlockOrder,
    InvalidVersion,
    ConflictingDisplayEngines,
}

impl AsicIpInventory {
    /// Enforce the canonical order and basic consistency of a synthetic
    /// diagnostic snapshot. This validation does NOT attest the source of
    /// an IP version; real hardware evidence remains a separate gate.
    pub fn typed_contracts(self) -> Result<TypedIpContracts, IpContractError> {
        for (index, block) in self.blocks.iter().enumerate() {
            if block.kind != IP_KINDS[index] {
                return Err(IpContractError::IncorrectBlockOrder);
            }
            if matches!(block.discovery, IpDiscovery::PresentVerified(IpVersion { major: 0, .. })) {
                return Err(IpContractError::InvalidVersion);
            }
        }
        let display = DisplayIpContract {
            dce: self.blocks[0].discovery,
            dcn: self.blocks[1].discovery,
        };
        if matches!(display.dce, IpDiscovery::PresentVerified(_))
            && matches!(display.dcn, IpDiscovery::PresentVerified(_))
        {
            return Err(IpContractError::ConflictingDisplayEngines);
        }
        Ok(TypedIpContracts {
            family_hint: self.family_hint,
            display,
            gmc: GmcContract { version: self.blocks[2].discovery },
            psp: PspContract { version: self.blocks[3].discovery },
            smu: SmuContract { version: self.blocks[4].discovery },
            ih: IhContract { version: self.blocks[5].discovery },
            sdma: SdmaContract { version: self.blocks[6].discovery },
            gfx: GfxContract { version: self.blocks[7].discovery },
        })
    }
}

impl TypedIpContracts {
    /// A diagnostic record cannot implicitly enable GPU initialization.
    /// A3/A4 will need separately authenticated board/firmware/resource
    /// evidence and an explicit driver implementation.
    pub const fn may_initialize_hardware(self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_diagnostic_asic_starts_with_unverified_ip_blocks() {
        for family in [
            AsicFamily::Unknown,
            AsicFamily::Polaris10,
            AsicFamily::Navi10,
            AsicFamily::Navi21,
        ] {
            let inventory = AsicIpInventory::unknown(family);
            assert!(!inventory.all_blocks_verified());
            assert!(!inventory.has_verified_display_engine());
            for (i, block) in inventory.blocks.iter().enumerate() {
                assert_eq!(block.kind, IP_KINDS[i]);
                assert_eq!(block.discovery, IpDiscovery::Unknown);
            }
        }
    }

    #[test]
    fn independent_ip_contracts_keep_unknown_values_for_every_asic() {
        for family in [AsicFamily::Unknown, AsicFamily::Polaris10,
                       AsicFamily::Navi10, AsicFamily::Navi21] {
            let typed = AsicIpInventory::unknown(family).typed_contracts().unwrap();
            assert_eq!(typed.family_hint, family);
            assert_eq!(typed.display.dce, IpDiscovery::Unknown);
            assert_eq!(typed.display.dcn, IpDiscovery::Unknown);
            assert_eq!(typed.gmc.version, IpDiscovery::Unknown);
            assert_eq!(typed.psp.version, IpDiscovery::Unknown);
            assert_eq!(typed.smu.version, IpDiscovery::Unknown);
            assert_eq!(typed.ih.version, IpDiscovery::Unknown);
            assert_eq!(typed.sdma.version, IpDiscovery::Unknown);
            assert_eq!(typed.gfx.version, IpDiscovery::Unknown);
            assert!(!typed.may_initialize_hardware());
        }
    }

    #[test]
    fn malformed_ip_metadata_cannot_become_a_valid_asic_profile() {
        let mut ip = AsicIpInventory::unknown(AsicFamily::Navi21);
        ip.blocks.swap(0, 2);
        assert_eq!(ip.typed_contracts(), Err(IpContractError::IncorrectBlockOrder));
        let mut ip = AsicIpInventory::unknown(AsicFamily::Navi21);
        ip.blocks[2].discovery = IpDiscovery::PresentVerified(IpVersion {
            major: 0, minor: 1, revision: 0,
        });
        assert_eq!(ip.typed_contracts(), Err(IpContractError::InvalidVersion));
        let mut ip = AsicIpInventory::unknown(AsicFamily::Navi21);
        let version = IpDiscovery::PresentVerified(IpVersion {
            major: 3, minor: 0, revision: 0,
        });
        ip.blocks[0].discovery = version;
        ip.blocks[1].discovery = version;
        assert_eq!(ip.typed_contracts(), Err(IpContractError::ConflictingDisplayEngines));
    }

    #[test]
    fn both_display_engines_cannot_be_considered_a_single_verified_path() {
        let mut ip = AsicIpInventory::unknown(AsicFamily::Navi21);
        let version = IpVersion {
            major: 3,
            minor: 0,
            revision: 0,
        };
        ip.blocks[0].discovery = IpDiscovery::PresentVerified(version);
        ip.blocks[1].discovery = IpDiscovery::PresentVerified(version);
        assert!(!ip.has_verified_display_engine());
    }
}
