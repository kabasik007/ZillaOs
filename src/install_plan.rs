//! Pure, **read-only** partition layout planning for a future ZillaOS installer.
//!
//! This module does not enumerate disks, open devices, issue storage commands,
//! create GPT records, format filesystems, mount volumes or install boot code.
//! It accepts caller-supplied geometry and computes a preview only.
//! NEVER turn this function into a write operation or auto-select a disk.

pub const MIB: u64 = 1024 * 1024;
pub const GIB: u64 = 1024 * MIB;
pub const ESP_BYTES: u64 = 512 * MIB;
pub const MIN_SYSTEM_BYTES: u64 = 8 * GIB;
pub const END_GUARD_BYTES: u64 = MIB;
pub const START_GUARD_BYTES: u64 = MIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    ZeroCapacity,
    UnsupportedLogicalSector,
    CapacityNotSectorAligned,
    TooSmall,
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskGeometry {
    /// Bytes reported by a future trusted block driver, never auto-detected here.
    pub capacity_bytes: u64,
    /// Only 512-byte and 4096-byte logical blocks are currently modeled.
    pub logical_sector_bytes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionPurpose {
    EfiSystem,
    ZillaSystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartitionSpan {
    /// Inclusive first logical block number.
    pub first_lba: u64,
    /// Number of logical blocks. Last LBA is first_lba + sectors - 1.
    pub sectors: u64,
    pub purpose: PartitionPurpose,
}

impl PartitionSpan {
    pub fn end_exclusive(self) -> Option<u64> {
        self.first_lba.checked_add(self.sectors)
    }

    pub fn size_bytes(self, logical_sector_bytes: u32) -> Option<u64> {
        self.sectors.checked_mul(logical_sector_bytes as u64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewPlan {
    pub logical_sector_bytes: u32,
    pub total_sectors: u64,
    pub efi: PartitionSpan,
    pub system: PartitionSpan,
}

impl PreviewPlan {
    /// Only reports geometry; this type deliberately has NO execute method.
    pub fn fits_in(self, disk: DiskGeometry) -> bool {
        let sector = disk.logical_sector_bytes as u64;
        sector != 0
            && disk.capacity_bytes % sector == 0
            && self.logical_sector_bytes == disk.logical_sector_bytes
            && disk.capacity_bytes / sector == self.total_sectors
            && self.efi.end_exclusive() == Some(self.system.first_lba)
            && self.system.end_exclusive().is_some_and(|end| {
                end <= self.total_sectors.saturating_sub(END_GUARD_BYTES / sector)
            })
    }
}

/// Produce a deterministic PREVIEW for a *hypothetical* dedicated blank disk.
///
/// The layout reserves the first MiB for protective MBR/primary GPT and leaves
/// the final MiB unused for GPT backup metadata, then proposes a 512-MiB ESP
/// and an 8-GiB-or-larger system partition. Real data-loss checks, disk-id
/// confirmations, GPT creation, filesystem writing and rollback are NOT here.
pub fn preview_for(disk: DiskGeometry) -> Result<PreviewPlan, PlanError> {
    if disk.capacity_bytes == 0 {
        return Err(PlanError::ZeroCapacity);
    }
    if disk.logical_sector_bytes != 512 && disk.logical_sector_bytes != 4096 {
        return Err(PlanError::UnsupportedLogicalSector);
    }
    let sector = disk.logical_sector_bytes as u64;
    if disk.capacity_bytes % sector != 0 {
        return Err(PlanError::CapacityNotSectorAligned);
    }

    let sectors = disk.capacity_bytes / sector;
    let start_guard = START_GUARD_BYTES / sector;
    let end_guard = END_GUARD_BYTES / sector;
    let efi_sectors = ESP_BYTES / sector;
    let min_root = MIN_SYSTEM_BYTES / sector;
    let efi_start = start_guard;
    let root_start = efi_start.checked_add(efi_sectors)
        .ok_or(PlanError::ArithmeticOverflow)?;
    let root_end = sectors.checked_sub(end_guard).ok_or(PlanError::TooSmall)?;
    let root_sectors = root_end.checked_sub(root_start).ok_or(PlanError::TooSmall)?;

    if root_sectors < min_root {
        return Err(PlanError::TooSmall);
    }
    let plan = PreviewPlan {
        logical_sector_bytes: disk.logical_sector_bytes,
        total_sectors: sectors,
        efi: PartitionSpan {
            first_lba: efi_start,
            sectors: efi_sectors,
            purpose: PartitionPurpose::EfiSystem,
        },
        system: PartitionSpan {
            first_lba: root_start,
            sectors: root_sectors,
            purpose: PartitionPurpose::ZillaSystem,
        },
    };
    if !plan.fits_in(disk) {
        return Err(PlanError::ArithmeticOverflow);
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_32_gib_disk_generates_aligned_nonoverlapping_plan() {
        for sector in [512, 4096] {
            let disk = DiskGeometry {
                capacity_bytes: 32 * GIB,
                logical_sector_bytes: sector,
            };
            let p = preview_for(disk).unwrap();
            assert!(p.fits_in(disk));
            assert_eq!(p.efi.first_lba * sector as u64, MIB);
            assert_eq!(p.efi.size_bytes(sector), Some(ESP_BYTES));
            assert_eq!(p.system.first_lba * sector as u64, MIB + ESP_BYTES);
            assert!(p.system.size_bytes(sector).unwrap() >= MIN_SYSTEM_BYTES);
            assert_eq!(p.system.end_exclusive().unwrap() * sector as u64, 32 * GIB - MIB);
        }
    }

    #[test]
    fn insufficient_or_invalid_disks_are_rejected() {
        assert_eq!(preview_for(DiskGeometry {
            capacity_bytes: 0, logical_sector_bytes: 512,
        }), Err(PlanError::ZeroCapacity));
        assert_eq!(preview_for(DiskGeometry {
            capacity_bytes: 6 * GIB, logical_sector_bytes: 512,
        }), Err(PlanError::TooSmall));
        assert_eq!(preview_for(DiskGeometry {
            capacity_bytes: 20 * GIB, logical_sector_bytes: 520,
        }), Err(PlanError::UnsupportedLogicalSector));
        assert_eq!(preview_for(DiskGeometry {
            capacity_bytes: 16 * GIB + 1, logical_sector_bytes: 4096,
        }), Err(PlanError::CapacityNotSectorAligned));
    }

    #[test]
    fn minimum_disk_rounding_never_overlaps_gpt_reserves() {
        let disk = DiskGeometry {
            capacity_bytes: MIN_SYSTEM_BYTES + ESP_BYTES + 2 * MIB,
            logical_sector_bytes: 4096,
        };
        let plan = preview_for(disk).unwrap();
        assert_eq!(plan.system.size_bytes(4096), Some(MIN_SYSTEM_BYTES));
        assert_eq!(plan.efi.end_exclusive(), Some(plan.system.first_lba));
        assert!(plan.fits_in(disk));
    }

    #[test]
    fn huge_u64_geometry_is_handled_without_overflow() {
        let disk = DiskGeometry {
            capacity_bytes: u64::MAX - (u64::MAX % 4096),
            logical_sector_bytes: 4096,
        };
        let plan = preview_for(disk).unwrap();
        assert!(plan.fits_in(disk));
    }

    #[test]
    fn mismatched_non_aligned_capacity_cannot_reuse_plan() {
        let disk = DiskGeometry {
            capacity_bytes: 32 * GIB,
            logical_sector_bytes: 512,
        };
        let plan = preview_for(disk).unwrap();
        assert!(!plan.fits_in(DiskGeometry {
            capacity_bytes: 32 * GIB + 1,
            logical_sector_bytes: 512,
        }));
    }

    #[test]
    fn cannot_relabel_a_plan_for_another_disk() {
        let first = DiskGeometry {
            capacity_bytes: 32 * GIB,
            logical_sector_bytes: 512,
        };
        let second = DiskGeometry {
            capacity_bytes: 16 * GIB,
            logical_sector_bytes: 512,
        };
        let p = preview_for(first).unwrap();
        assert!(!p.fits_in(second));
    }
}
