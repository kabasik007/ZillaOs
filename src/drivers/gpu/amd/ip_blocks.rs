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
