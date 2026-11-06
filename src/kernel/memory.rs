//! Early physical memory map validation and 4 KiB frame reservations.
//! No heap allocation, no access to the returned physical addresses.
//! This deliberately remains a bump-only planner until paging/ownership
//! support exists; never use it to free pages or reassign DMA memory.

pub const PAGE_SIZE: u64 = 4096;
pub const MAX_MEMORY_REGIONS: usize = 256;
const MAX_PHYSICAL_ADDRESS_BITS: u8 = 52;
const MIN_PHYSICAL_ADDRESS_BITS: u8 = 32;
const USABLE_KIND: u64 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub kind: u64,
}

impl MemoryRegion {
    pub const EMPTY: Self = Self {
        base: 0,
        length: 0,
        kind: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryError {
    EmptyMap,
    TooManyRegions,
    EmptyRegion,
    AddressOverflow,
    PhysicalAddressOutOfRange,
    InvalidPhysicalAddressWidth,
    OverlappingRegions,
    NoUsableFrames,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalFrame {
    /// Start of a 4096-byte-aligned physical frame.
    pub address: u64,
}

#[derive(Clone, Copy)]
struct Range {
    start: u64,
    end: u64,
}

impl Range {
    const EMPTY: Self = Self { start: 0, end: 0 };
}

pub struct FrameAllocator {
    ranges: [Range; MAX_MEMORY_REGIONS],
    range_count: usize,
    cursor_region: usize,
    cursor_address: u64,
    available_frames: u64,
}

impl FrameAllocator {
    /// Validate a complete bootloader map and retain *only* usable page ranges.
    ///
    /// Unknown memory types and all bootloader-reclaimable/ACPI regions are
    /// reserved. The lowest physical page is also intentionally reserved.
    ///
    /// Overlap checks are O(n²), bounded by MAX_MEMORY_REGIONS.
    pub fn from_regions(regions: &[MemoryRegion]) -> Result<Self, MemoryError> {
        Self::from_regions_with_physical_bits(regions, MAX_PHYSICAL_ADDRESS_BITS)
    }

    /// Bound allocation to the usable physical-address width reported by CPUID.
    /// Reserve all non-USABLE regions, including firmware, DMA and framebuffer.
    pub fn from_regions_with_physical_bits(
        regions: &[MemoryRegion],
        physical_bits: u8,
    ) -> Result<Self, MemoryError> {
        if !(MIN_PHYSICAL_ADDRESS_BITS..=MAX_PHYSICAL_ADDRESS_BITS).contains(&physical_bits) {
            return Err(MemoryError::InvalidPhysicalAddressWidth);
        }
        let physical_limit = 1u64 << physical_bits;
        if regions.is_empty() {
            return Err(MemoryError::EmptyMap);
        }
        if regions.len() > MAX_MEMORY_REGIONS {
            return Err(MemoryError::TooManyRegions);
        }

        let mut available_frames = 0u64;
        let mut ranges = [Range::EMPTY; MAX_MEMORY_REGIONS];
        let mut range_count = 0usize;

        for (index, region) in regions.iter().enumerate() {
            if region.length == 0 {
                return Err(MemoryError::EmptyRegion);
            }
            let end = region
                .base
                .checked_add(region.length)
                .ok_or(MemoryError::AddressOverflow)?;
            if region.kind == USABLE_KIND && end > physical_limit {
                return Err(MemoryError::PhysicalAddressOutOfRange);
            }

            // Limine permits non-USABLE regions to overlap each other.
            // Only USABLE overlaps would risk allocating reserved physical RAM.
            for prior in &regions[..index] {
                let prior_end = prior
                    .base
                    .checked_add(prior.length)
                    .ok_or(MemoryError::AddressOverflow)?;
                if (region.kind == USABLE_KIND || prior.kind == USABLE_KIND)
                    && region.base < prior_end
                    && prior.base < end
                {
                    return Err(MemoryError::OverlappingRegions);
                }
            }

            if region.kind != USABLE_KIND {
                continue;
            }

            let begin = region.base.max(PAGE_SIZE);
            let start = begin
                .checked_add(PAGE_SIZE - 1)
                .ok_or(MemoryError::AddressOverflow)?
                & !(PAGE_SIZE - 1);
            let aligned_end = end & !(PAGE_SIZE - 1);
            if start >= aligned_end {
                continue;
            }
            let pages = (aligned_end - start) / PAGE_SIZE;
            available_frames = available_frames
                .checked_add(pages)
                .ok_or(MemoryError::AddressOverflow)?;

            // Insert sorted by physical start. No heap and no external crate.
            let mut position = range_count;
            while position > 0 && ranges[position - 1].start > start {
                ranges[position] = ranges[position - 1];
                position -= 1;
            }
            ranges[position] = Range {
                start,
                end: aligned_end,
            };
            range_count += 1;
        }

        if range_count == 0 || available_frames == 0 {
            return Err(MemoryError::NoUsableFrames);
        }
        let cursor_address = ranges[0].start;
        Ok(Self {
            ranges,
            range_count,
            cursor_region: 0,
            cursor_address,
            available_frames,
        })
    }

    pub const fn remaining_frames(&self) -> u64 {
        self.available_frames
    }

    /// Reserve a physical frame; callers must arrange virtual mapping before
    /// accessing the address. This early allocator deliberately cannot free.
    pub fn reserve_frame(&mut self) -> Option<PhysicalFrame> {
        while self.cursor_region < self.range_count {
            let range = self.ranges[self.cursor_region];
            if self.cursor_address < range.end {
                let frame = PhysicalFrame {
                    address: self.cursor_address,
                };
                self.cursor_address += PAGE_SIZE;
                self.available_frames -= 1;
                return Some(frame);
            }
            self.cursor_region += 1;
            if self.cursor_region < self.range_count {
                self.cursor_address = self.ranges[self.cursor_region].start;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn region(base: u64, length: u64, kind: u64) -> MemoryRegion {
        MemoryRegion { base, length, kind }
    }

    #[test]
    fn basic_frame_reservation_is_aligned_and_unique() {
        let mut alloc = FrameAllocator::from_regions(&[
            region(0, 16 * PAGE_SIZE, 0),
            region(32 * PAGE_SIZE, 4 * PAGE_SIZE, 1),
            region(64 * PAGE_SIZE, 3 * PAGE_SIZE, 0),
        ])
        .unwrap();
        assert_eq!(alloc.remaining_frames(), 18);
        let mut previous = 0;
        for _ in 0..18 {
            let next = alloc.reserve_frame().unwrap().address;
            assert_eq!(next % PAGE_SIZE, 0);
            assert!(next > previous);
            assert!((PAGE_SIZE..16 * PAGE_SIZE).contains(&next)
                || (64 * PAGE_SIZE..67 * PAGE_SIZE).contains(&next));
            previous = next;
        }
        assert_eq!(alloc.remaining_frames(), 0);
        assert!(alloc.reserve_frame().is_none());
    }

    #[test]
    fn unsorted_regions_are_sorted_before_allocation() {
        let mut alloc = FrameAllocator::from_regions(&[
            region(20 * PAGE_SIZE, 2 * PAGE_SIZE, 0),
            region(8 * PAGE_SIZE, 2 * PAGE_SIZE, 0),
        ])
        .unwrap();
        assert_eq!(alloc.reserve_frame().unwrap().address, 8 * PAGE_SIZE);
        assert_eq!(alloc.reserve_frame().unwrap().address, 9 * PAGE_SIZE);
        assert_eq!(alloc.reserve_frame().unwrap().address, 20 * PAGE_SIZE);
    }

    #[test]
    fn partial_pages_are_never_allocated() {
        let mut alloc = FrameAllocator::from_regions(&[region(
            PAGE_SIZE + 3,
            2 * PAGE_SIZE,
            0,
        )])
        .unwrap();
        assert_eq!(alloc.remaining_frames(), 1);
        assert_eq!(alloc.reserve_frame().unwrap().address, 2 * PAGE_SIZE);
        assert!(alloc.reserve_frame().is_none());
    }

    #[test]
    fn overlapping_reserved_region_is_rejected() {
        assert!(matches!(
            FrameAllocator::from_regions(&[
                region(0x1000, 0x8000, 0),
                region(0x2000, 0x1000, 1),
            ]),
            Err(MemoryError::OverlappingRegions)
        ));
    }

    #[test]
    fn integer_wrap_and_empty_region_are_rejected() {
        assert!(matches!(
            FrameAllocator::from_regions(&[region(u64::MAX - 100, 1000, 0)]),
            Err(MemoryError::AddressOverflow)
        ));
        assert!(matches!(
            FrameAllocator::from_regions(&[region(0, 0, 0)]),
            Err(MemoryError::EmptyRegion)
        ));
    }

    #[test]
    fn zero_page_and_reserved_only_are_never_used() {
        assert!(matches!(
            FrameAllocator::from_regions(&[region(0, PAGE_SIZE, 0)]),
            Err(MemoryError::NoUsableFrames)
        ));
        assert!(matches!(
            FrameAllocator::from_regions(&[region(0x1000, 0x100000, 5)]),
            Err(MemoryError::NoUsableFrames)
        ));
        assert!(matches!(
            FrameAllocator::from_regions(&[region(0x1000, 0x100000, 99)]),
            Err(MemoryError::NoUsableFrames)
        ));
    }

    #[test]
    fn valid_but_unmapped_high_physical_addresses_are_rejected() {
        assert!(matches!(
            FrameAllocator::from_regions(&[region((1u64 << 52) - PAGE_SIZE, PAGE_SIZE * 2, 0)]),
            Err(MemoryError::PhysicalAddressOutOfRange)
        ));
    }

    #[test]
    fn overlapping_firmware_and_reserved_regions_are_allowed() {
        let mut allocator = FrameAllocator::from_regions(&[
            region(0x1000, 0x2000, 2),
            region(0x2000, 0x2000, 1),
            region(0x10000, 0x4000, 0),
        ]).unwrap();
        assert_eq!(allocator.remaining_frames(), 4);
        assert_eq!(allocator.reserve_frame().unwrap().address, 0x10000);
    }

    #[test]
    fn usable_overlap_with_reserved_region_is_rejected() {
        assert!(matches!(
            FrameAllocator::from_regions(&[
                region(0x2000, 0x4000, 1),
                region(0x4000, 0x4000, 0),
            ]),
            Err(MemoryError::OverlappingRegions)
        ));
    }

    #[test]
    fn physical_address_width_is_enforced_for_usable_frames() {
        let region_at_4gb = region(1u64 << 32, PAGE_SIZE, 0);
        assert!(matches!(
            FrameAllocator::from_regions_with_physical_bits(&[region_at_4gb], 32),
            Err(MemoryError::PhysicalAddressOutOfRange)
        ));
        let mut alloc = FrameAllocator::from_regions_with_physical_bits(
            &[region_at_4gb], 36
        ).unwrap();
        assert_eq!(alloc.reserve_frame().unwrap().address, 1u64 << 32);
        assert!(matches!(
            FrameAllocator::from_regions_with_physical_bits(
                &[region(0x1000, PAGE_SIZE, 0)], 0
            ),
            Err(MemoryError::InvalidPhysicalAddressWidth)
        ));
    }

    #[test]
    fn too_many_regions_and_empty_map_are_rejected() {
        assert!(matches!(
            FrameAllocator::from_regions(&[]),
            Err(MemoryError::EmptyMap)
        ));
        let data = [region(0x2000, PAGE_SIZE, 1); MAX_MEMORY_REGIONS + 1];
        assert!(matches!(
            FrameAllocator::from_regions(&data),
            Err(MemoryError::TooManyRegions)
        ));
    }
}
