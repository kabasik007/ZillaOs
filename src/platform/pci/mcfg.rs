//! Strict, pure ACPI MCFG table parser (ACPI SDT header + 8 reserved bytes).
//! No physical memory dereference and no ECAM mapping is performed.
use super::identity::PciAddress;

const ACPI_HEADER: usize = 36;
const MCFG_HEADER: usize = ACPI_HEADER + 8;
const ALLOCATION_SIZE: usize = 16;
const BUS_STRIDE: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McfgError {
    TooShort,
    BadSignature,
    InvalidLength,
    BadChecksum,
    BadReserved,
    InvalidAllocation,
    UnalignedBase,
    OverlappingAllocations,
    Overflow,
    TooManyAllocations,
    OutOfRange,
    UnalignedOffset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McfgAllocation {
    pub base_address: u64,
    pub segment: u16,
    pub start_bus: u8,
    pub end_bus: u8,
}

impl McfgAllocation {
    pub fn physical_extent(self) -> Result<(u64, u64), McfgError> {
        // MCFG base is bus-0 based. The first byte mapped for a nonzero
        // start bus is base + (start_bus << 20), not the raw MCFG base.
        let Some(start) = self.base_address.checked_add((self.start_bus as u64) * BUS_STRIDE) else {
            return Err(McfgError::Overflow);
        };
        let Some(end) = self.base_address.checked_add(((self.end_bus as u64) + 1) * BUS_STRIDE) else {
            return Err(McfgError::Overflow);
        };
        Ok((start, end))
    }

    pub fn ecam_address(self, address: PciAddress, offset: u16) -> Result<u64, McfgError> {
        if !address.is_valid() || address.segment != self.segment
            || address.bus < self.start_bus || address.bus > self.end_bus {
            return Err(McfgError::OutOfRange);
        }
        if offset & 3 != 0 { return Err(McfgError::UnalignedOffset); }
        if offset > 4092 { return Err(McfgError::OutOfRange); }
        let addition = ((address.bus as u64) << 20)
            | ((address.device as u64) << 15)
            | ((address.function as u64) << 12)
            | (offset as u64);
        let Some(physical) = self.base_address.checked_add(addition) else {
            return Err(McfgError::Overflow);
        };
        let (_, end) = self.physical_extent()?;
        if physical > end - 4 { return Err(McfgError::OutOfRange); }
        Ok(physical)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McfgTable<const CAP: usize> {
    allocations: [Option<McfgAllocation>; CAP],
    pub count: usize,
}

impl<const CAP: usize> McfgTable<CAP> {
    pub const fn empty() -> Self { Self { allocations: [None; CAP], count: 0 } }
    pub fn iter(&self) -> impl Iterator<Item = McfgAllocation> + '_ {
        self.allocations[..self.count].iter().flatten().copied()
    }

    pub fn ecam_address(&self, address: PciAddress, offset: u16) -> Result<u64, McfgError> {
        for allocation in self.iter() {
            if allocation.segment == address.segment
                && address.bus >= allocation.start_bus
                && address.bus <= allocation.end_bus {
                return allocation.ecam_address(address, offset);
            }
        }
        Err(McfgError::OutOfRange)
    }
}

fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}
fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset], bytes[offset+1], bytes[offset+2], bytes[offset+3],
        bytes[offset+4], bytes[offset+5], bytes[offset+6], bytes[offset+7],
    ])
}

/// Validate MCFG signature, declared table length, checksum and every entry.
/// Firmware physical pointers are NOT accepted: caller must provide a
/// previously bounded/copy-validated byte slice from trusted ACPI discovery.
pub fn parse_mcfg<const CAP: usize>(bytes: &[u8]) -> Result<McfgTable<CAP>, McfgError> {
    if bytes.len() < MCFG_HEADER { return Err(McfgError::TooShort); }
    if &bytes[..4] != b"MCFG" { return Err(McfgError::BadSignature); }
    let declared = get_u32(bytes, 4) as usize;
    if declared < MCFG_HEADER || declared > bytes.len()
        || (declared - MCFG_HEADER) % ALLOCATION_SIZE != 0 {
        return Err(McfgError::InvalidLength);
    }
    let bytes = &bytes[..declared];
    if bytes.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)) != 0 {
        return Err(McfgError::BadChecksum);
    }
    if bytes[ACPI_HEADER..MCFG_HEADER].iter().any(|b| *b != 0) {
        return Err(McfgError::BadReserved);
    }
    let mut result = McfgTable::<CAP>::empty();
    for chunk in bytes[MCFG_HEADER..].chunks_exact(ALLOCATION_SIZE) {
        if result.count == CAP { return Err(McfgError::TooManyAllocations); }
        if chunk[12..16].iter().any(|b| *b != 0) { return Err(McfgError::BadReserved); }
        let alloc = McfgAllocation {
            base_address: get_u64(chunk, 0),
            segment: get_u16(chunk, 8),
            start_bus: chunk[10],
            end_bus: chunk[11],
        };
        if alloc.start_bus > alloc.end_bus || alloc.base_address == 0 {
            return Err(McfgError::InvalidAllocation);
        }
        if alloc.base_address & (BUS_STRIDE - 1) != 0 {
            return Err(McfgError::UnalignedBase);
        }
        let (start, end) = alloc.physical_extent()?;
        for existing in result.iter() {
            let (old_start, old_end) = existing.physical_extent()?;
            let bus_overlap = alloc.segment == existing.segment
                && alloc.start_bus <= existing.end_bus && existing.start_bus <= alloc.end_bus;
            let address_overlap = start < old_end && old_start < end;
            if bus_overlap || address_overlap {
                return Err(McfgError::OverlappingAllocations);
            }
        }
        result.allocations[result.count] = Some(alloc);
        result.count += 1;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn valid_mcfg(allocations: &[(u64, u16, u8, u8)]) -> Vec<u8> {
        let mut table = std::vec![0u8; 44 + 16 * allocations.len()];
        table[..4].copy_from_slice(b"MCFG");
        let len = table.len() as u32;
        table[4..8].copy_from_slice(&len.to_le_bytes());
        table[8] = 1; // revision
        for (i, (base, seg, start, end)) in allocations.iter().copied().enumerate() {
            let at = 44 + 16 * i;
            table[at..at+8].copy_from_slice(&base.to_le_bytes());
            table[at+8..at+10].copy_from_slice(&seg.to_le_bytes());
            table[at+10] = start;
            table[at+11] = end;
        }
        rechecksum(&mut table);
        table
    }
    fn rechecksum(table: &mut [u8]) {
        table[9] = 0;
        let checksum = table.iter().fold(0u8, |sum,b| sum.wrapping_add(*b));
        table[9] = 0u8.wrapping_sub(checksum);
    }

    #[test]
    fn accepts_valid_mcfg_and_uses_absolute_bus_number() {
        let data = valid_mcfg(&[(0xe000_0000, 0, 32, 40), (0x1_0000_0000, 1, 0, 0)]);
        let parsed = parse_mcfg::<4>(&data).unwrap();
        assert_eq!(parsed.count, 2);
        let addr = PciAddress::new(0, 33, 31, 7).unwrap();
        assert_eq!(parsed.ecam_address(addr, 0xffc).unwrap(),
            0xe000_0000 + (33u64 << 20) + (31u64 << 15) + (7u64 << 12) + 0xffc);
        assert_eq!(parsed.ecam_address(PciAddress::new(1,0,0,0).unwrap(), 0), Ok(0x1_0000_0000));
        assert_eq!(parsed.ecam_address(addr, 1), Err(McfgError::UnalignedOffset));
        assert_eq!(parsed.ecam_address(addr, 4096), Err(McfgError::OutOfRange));
    }

    #[test]
    fn denies_truncation_bad_length_and_checksum() {
        assert_eq!(parse_mcfg::<1>(&[]), Err(McfgError::TooShort));
        let mut data = valid_mcfg(&[(0xe000_0000,0,0,1)]);
        data[0] = b'X';
        assert_eq!(parse_mcfg::<1>(&data), Err(McfgError::BadSignature));
        data = valid_mcfg(&[(0xe000_0000,0,0,1)]);
        data[9] ^= 1;
        assert_eq!(parse_mcfg::<1>(&data), Err(McfgError::BadChecksum));
        data = valid_mcfg(&[(0xe000_0000,0,0,1)]);
        data[4] = 59; // declared length not a multiple of 16
        assert_eq!(parse_mcfg::<1>(&data), Err(McfgError::InvalidLength));
    }

    #[test]
    fn overlapping_and_invalid_allocations_fail_closed() {
        let data = valid_mcfg(&[(0xe000_0000,0,0,4),(0xe000_0000,0,3,5)]);
        assert_eq!(parse_mcfg::<4>(&data), Err(McfgError::OverlappingAllocations));
        let data = valid_mcfg(&[(0xe000_0000,0,0,4),(0xe000_0000,1,0,4)]);
        assert_eq!(parse_mcfg::<4>(&data), Err(McfgError::OverlappingAllocations));
        let data = valid_mcfg(&[(0xe000_0000,0,8,4)]);
        assert_eq!(parse_mcfg::<1>(&data), Err(McfgError::InvalidAllocation));
        let data = valid_mcfg(&[(0xe000_0001,0,0,4)]);
        assert_eq!(parse_mcfg::<1>(&data), Err(McfgError::UnalignedBase));
        let data = valid_mcfg(&[(u64::MAX & !0xfffff,0,0,255)]);
        assert_eq!(parse_mcfg::<1>(&data), Err(McfgError::Overflow));
        let data = valid_mcfg(&[(0xe000_0000,0,0,4)]);
        assert_eq!(parse_mcfg::<0>(&data), Err(McfgError::TooManyAllocations));
    }

    #[test]
    fn reserved_fields_and_unsupported_bus_rejected() {
        let mut data = valid_mcfg(&[(0xe000_0000,0,10,11)]);
        data[40] = 1;
        rechecksum(&mut data);
        assert_eq!(parse_mcfg::<1>(&data), Err(McfgError::BadReserved));
        let table = parse_mcfg::<1>(&valid_mcfg(&[(0xe000_0000,0,10,11)])).unwrap();
        assert_eq!(table.ecam_address(PciAddress::new(0,12,0,0).unwrap(),0), Err(McfgError::OutOfRange));
        assert_eq!(table.ecam_address(PciAddress::new(2,10,0,0).unwrap(),0), Err(McfgError::OutOfRange));
    }
}
