//! Host-testable user virtual-address layout policy.
//! A plan is NOT an installed page table or an isolation boundary.
pub const PAGE_SIZE: u64 = 4096;
pub const USER_MIN: u64 = 0x0001_0000;
pub const USER_TOP: u64 = 0x0000_8000_0000_0000;
pub const MAX_REGIONS: usize = 32;
pub const MAX_PAGES: u64 = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    InvalidRange,
    Unaligned,
    InvalidPermissions,
    GuardPage,
    Overlap,
    TooManyRegions,
    PageLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Execute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Permissions {
    pub const READ_ONLY: Self = Self { read: true, write: false, execute: false };
    pub const READ_WRITE: Self = Self { read: true, write: true, execute: false };
    pub const READ_EXECUTE: Self = Self { read: true, write: false, execute: true };

    pub fn checked(read: bool, write: bool, execute: bool) -> Result<Self, MapError> {
        if (!read && (write || execute)) || (write && execute) {
            return Err(MapError::InvalidPermissions);
        }
        Ok(Self { read, write, execute })
    }

    fn allows(self, access: Access) -> bool {
        match access {
            Access::Read => self.read,
            Access::Write => self.write,
            Access::Execute => self.execute,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserRegion {
    pub base: u64,
    pub size: u64,
    pub permissions: Permissions,
}

impl UserRegion {
    pub fn checked(base: u64, size: u64, permissions: Permissions) -> Result<Self, MapError> {
        let end = base.checked_add(size).ok_or(MapError::InvalidRange)?;
        if size == 0 || base < USER_MIN || end > USER_TOP || end <= base {
            return Err(MapError::InvalidRange);
        }
        if base % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return Err(MapError::Unaligned);
        }
        Permissions::checked(permissions.read, permissions.write, permissions.execute)?;
        Ok(Self { base, size, permissions })
    }

    pub fn end(self) -> u64 {
        self.base + self.size // validated by checked()
    }

    fn overlaps(self, other: Self) -> bool {
        self.base < other.end() && other.base < self.end()
    }
}

/// Fixed-size metadata; must be committed to actual U/S+NX page tables before execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddressSpacePlan {
    regions: [Option<UserRegion>; MAX_REGIONS],
    count: usize,
    pages: u64,
    stack_top: u64,
    guard_base: u64,
}

impl AddressSpacePlan {
    pub fn with_stack(stack_top: u64, stack_pages: u64) -> Result<Self, MapError> {
        if stack_top % PAGE_SIZE != 0 || stack_pages == 0 || stack_pages >= MAX_PAGES {
            return Err(MapError::InvalidRange);
        }
        let stack_bytes = stack_pages.checked_mul(PAGE_SIZE).ok_or(MapError::InvalidRange)?;
        let bottom = stack_top.checked_sub(stack_bytes).ok_or(MapError::InvalidRange)?;
        let guard_base = bottom.checked_sub(PAGE_SIZE).ok_or(MapError::InvalidRange)?;
        if guard_base < USER_MIN || stack_top > USER_TOP {
            return Err(MapError::InvalidRange);
        }
        let mut plan = Self {
            regions: [None; MAX_REGIONS],
            count: 0,
            pages: 0,
            stack_top,
            guard_base,
        };
        plan.map(UserRegion::checked(bottom, stack_bytes, Permissions::READ_WRITE)?)?;
        Ok(plan)
    }

    pub fn map(&mut self, region: UserRegion) -> Result<(), MapError> {
        let region = UserRegion::checked(region.base, region.size, region.permissions)?;
        if region.base < self.guard_base + PAGE_SIZE && region.end() > self.guard_base {
            return Err(MapError::GuardPage);
        }
        if self.regions[..self.count].iter().flatten().any(|old| old.overlaps(region)) {
            return Err(MapError::Overlap);
        }
        if self.count == MAX_REGIONS {
            return Err(MapError::TooManyRegions);
        }
        let pages = self.pages.checked_add(region.size / PAGE_SIZE).ok_or(MapError::PageLimit)?;
        if pages > MAX_PAGES {
            return Err(MapError::PageLimit);
        }
        self.regions[self.count] = Some(region);
        self.count += 1;
        self.pages = pages;
        Ok(())
    }

    pub fn allows(&self, base: u64, len: u64, access: Access) -> bool {
        let Some(end) = base.checked_add(len) else { return false };
        if len == 0 || base < USER_MIN || end > USER_TOP {
            return false;
        }
        self.regions[..self.count].iter().flatten().any(|region| {
            base >= region.base && end <= region.end() && region.permissions.allows(access)
        })
    }

    pub const fn pages(&self) -> u64 { self.pages }
    pub const fn stack_top(&self) -> u64 { self.stack_top }
    pub const fn guard_base(&self) -> u64 { self.guard_base }
    pub fn regions(&self) -> impl Iterator<Item = UserRegion> + '_ {
        self.regions[..self.count].iter().flatten().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_has_unmapped_guard_and_only_rw_pages() {
        let s = AddressSpacePlan::with_stack(0x800000, 4).unwrap();
        assert!(s.allows(0x7fffff, 1, Access::Write));
        assert!(!s.allows(0x7fffff, 1, Access::Execute));
        assert!(!s.allows(s.guard_base(), 1, Access::Read));
        assert_eq!(s.pages(), 4);
    }

    #[test]
    fn rejects_kernel_ranges_overflows_and_wx() {
        assert!(UserRegion::checked(USER_TOP, PAGE_SIZE, Permissions::READ_ONLY).is_err());
        assert!(UserRegion::checked(u64::MAX - 4095, PAGE_SIZE, Permissions::READ_ONLY).is_err());
        assert_eq!(Permissions::checked(true, true, true), Err(MapError::InvalidPermissions));
        assert_eq!(Permissions::checked(false, true, false), Err(MapError::InvalidPermissions));
    }

    #[test]
    fn rejects_guard_and_overlap_and_unaligned_regions() {
        let mut s = AddressSpacePlan::with_stack(0x800000, 4).unwrap();
        assert_eq!(
            s.map(UserRegion::checked(s.guard_base(), PAGE_SIZE, Permissions::READ_ONLY).unwrap()),
            Err(MapError::GuardPage)
        );
        let text = UserRegion::checked(0x400000, PAGE_SIZE, Permissions::READ_EXECUTE).unwrap();
        s.map(text).unwrap();
        assert_eq!(s.map(text), Err(MapError::Overlap));
        assert_eq!(
            UserRegion::checked(0x400001, PAGE_SIZE, Permissions::READ_ONLY),
            Err(MapError::Unaligned)
        );
        assert!(!s.allows(0x400fff, 2, Access::Execute));
        assert!(s.allows(0x400000, PAGE_SIZE, Access::Execute));
    }

    #[test]
    fn page_budget_and_mapping_capacity_are_enforced() {
        assert!(AddressSpacePlan::with_stack(0x800000, MAX_PAGES).is_err());
        let mut s = AddressSpacePlan::with_stack(0x800000, 1).unwrap();
        assert_eq!(
            s.map(UserRegion::checked(0x1000000, MAX_PAGES * PAGE_SIZE, Permissions::READ_ONLY).unwrap()),
            Err(MapError::PageLimit)
        );
        for n in 0..MAX_REGIONS - 1 {
            s.map(UserRegion::checked(0x10000 + (n as u64) * PAGE_SIZE, PAGE_SIZE, Permissions::READ_ONLY).unwrap()).unwrap();
        }
        assert_eq!(
            s.map(UserRegion::checked(0x100000, PAGE_SIZE, Permissions::READ_ONLY).unwrap()),
            Err(MapError::TooManyRegions)
        );
    }
}
