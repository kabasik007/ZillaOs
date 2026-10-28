//! Read-only GPT primary-header and partition-array verifier.
//!
//! This is pure Rust and has **no disk driver**. A future trusted block
//! service provides a callback that only reads logical sectors into a buffer.
//! No partition writes, formatting, PCI commands, disk selection or boot
//! installation are available from this module.
//!
//! V0 limitations: primary GPT only, 512/4096-byte logical blocks,
//! 128-byte entries, <=128 entries and <=32 populated partitions. Reject
//! anything else rather than interpreting unsupported on-disk structures.

pub const MAX_PARTITIONS: usize = 32;
pub const MAX_ENTRIES: u32 = 128;
const MAX_SECTOR: usize = 4096;
const GPT_HEADER_MIN: usize = 92;
const PARTITION_ENTRY_SIZE: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GptError {
    BadGeometry,
    ReadFailed,
    InvalidProtectiveMbr,
    HybridMbr,
    BadSignature,
    InvalidRevision,
    BadHeaderSize,
    HeaderCrcMismatch,
    WrongHeaderLocation,
    InvalidUsableRange,
    UnsupportedEntryLayout,
    InvalidArrayLocation,
    ArrayCrcMismatch,
    InvalidPartition,
    PartitionOverlap,
    TooManyPartitions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    /// Both endpoints inclusive.
    pub first_lba: u64,
    pub last_lba: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GptInfo {
    pub disk_guid: [u8; 16],
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub backup_header_lba: u64,
    pub partition_count: usize,
    pub partitions: [Option<Partition>; MAX_PARTITIONS],
}

impl GptInfo {
    pub fn iter(&self) -> impl Iterator<Item = Partition> + '_ {
        self.partitions[..self.partition_count].iter().flatten().copied()
    }
}

fn le32(bytes: &[u8], offset: usize) -> u32 {
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(raw)
}

fn le64(bytes: &[u8], offset: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(raw)
}

fn crc32_continue(mut state: u32, data: &[u8]) -> u32 {
    for byte in data {
        state ^= *byte as u32;
        for _ in 0..8 {
            state = (state >> 1) ^ (if state & 1 != 0 { 0xedb8_8320 } else { 0 });
        }
    }
    state
}

/// Standard GPT / IEEE reflected CRC-32 (not CRC32C).
pub fn crc32(data: &[u8]) -> u32 {
    !crc32_continue(!0u32, data)
}

/// Inspect a hypothetical block device through a strictly read-only callback.
///
/// The callback must fill `out` with exactly one logical sector from LBA and
/// return Err(()) on failure. The function uses a fixed 4-KiB stack buffer,
/// never allocates a disk image and never writes back through the callback.
/// The caller must provide a **verified** capacity and logical block size.
pub fn inspect_primary_gpt(
    sector_size: u32,
    total_sectors: u64,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), ()>,
) -> Result<GptInfo, GptError> {
    if !matches!(sector_size, 512 | 4096) || total_sectors < 6 {
        return Err(GptError::BadGeometry);
    }
    let block = sector_size as usize;
    let mut buffer = [0u8; MAX_SECTOR];

    // A GPT disk must advertise its protective MBR at LBA 0. Refuse a
    // hybrid MBR: unexpected legacy partitions need separate risk handling.
    read(0, &mut buffer[..block]).map_err(|_| GptError::ReadFailed)?;
    if buffer[510] != 0x55 || buffer[511] != 0xaa {
        return Err(GptError::InvalidProtectiveMbr);
    }
    let mut protective = 0usize;
    let protected_sectors = if total_sectors - 1 > u32::MAX as u64 {
        u32::MAX
    } else {
        (total_sectors - 1) as u32
    };
    for slot in 0..4 {
        let base = 446 + slot * 16;
        let kind = buffer[base + 4];
        if kind == 0xee {
            if le32(&buffer, base + 8) != 1 ||
                le32(&buffer, base + 12) != protected_sectors
            {
                return Err(GptError::InvalidProtectiveMbr);
            }
            protective += 1;
        } else if kind != 0 || buffer[base..base + 16].iter().any(|b| *b != 0) {
            return Err(GptError::HybridMbr);
        }
    }
    if protective != 1 {
        return Err(GptError::InvalidProtectiveMbr);
    }

    read(1, &mut buffer[..block]).map_err(|_| GptError::ReadFailed)?;
    if &buffer[..8] != b"EFI PART" {
        return Err(GptError::BadSignature);
    }
    if le32(&buffer, 8) != 0x0001_0000 {
        return Err(GptError::InvalidRevision);
    }
    let header_size = le32(&buffer, 12) as usize;
    if header_size < GPT_HEADER_MIN || header_size > block ||
        le32(&buffer, 20) != 0 ||
        buffer[header_size..block].iter().any(|b| *b != 0)
    {
        return Err(GptError::BadHeaderSize);
    }
    let expected_crc = le32(&buffer, 16);
    let mut crc_state = !0u32;
    crc_state = crc32_continue(crc_state, &buffer[..16]);
    crc_state = crc32_continue(crc_state, &[0u8; 4]);
    crc_state = crc32_continue(crc_state, &buffer[20..header_size]);
    if !crc_state != expected_crc {
        return Err(GptError::HeaderCrcMismatch);
    }

    let primary = le64(&buffer, 24);
    let backup = le64(&buffer, 32);
    if primary != 1 || backup != total_sectors - 1 {
        return Err(GptError::WrongHeaderLocation);
    }
    let first_usable = le64(&buffer, 40);
    let last_usable = le64(&buffer, 48);
    let mut disk_guid = [0u8; 16];
    disk_guid.copy_from_slice(&buffer[56..72]);
    if disk_guid == [0; 16] || first_usable > last_usable {
        return Err(GptError::InvalidUsableRange);
    }

    let table_lba = le64(&buffer, 72);
    let entry_count = le32(&buffer, 80);
    let entry_size = le32(&buffer, 84) as usize;
    let expected_table_crc = le32(&buffer, 88);
    if entry_count != MAX_ENTRIES || entry_size != PARTITION_ENTRY_SIZE
    {
        return Err(GptError::UnsupportedEntryLayout);
    }
    let table_bytes = (entry_count as u64)
        .checked_mul(entry_size as u64)
        .ok_or(GptError::UnsupportedEntryLayout)?;
    let table_sectors = table_bytes
        .checked_add(block as u64 - 1)
        .ok_or(GptError::InvalidArrayLocation)? / block as u64;
    let end_table = table_lba.checked_add(table_sectors)
        .ok_or(GptError::InvalidArrayLocation)?;
    let backup_table_start = backup.checked_sub(table_sectors)
        .ok_or(GptError::InvalidArrayLocation)?;
    if table_lba < 2 || end_table > first_usable ||
        first_usable > last_usable || last_usable >= backup_table_start ||
        end_table > backup_table_start
    {
        return Err(GptError::InvalidArrayLocation);
    }

    let mut info = GptInfo {
        disk_guid,
        first_usable_lba: first_usable,
        last_usable_lba: last_usable,
        backup_header_lba: backup,
        partition_count: 0,
        partitions: [None; MAX_PARTITIONS],
    };

    // Stream the array CRC and partition metadata through one scratch sector.
    // No heap and no implicit reads beyond the validated primary GPT array.
    let mut table_crc_state = !0u32;
    let mut remaining = table_bytes as usize;
    for sector_index in 0..table_sectors {
        let lba = table_lba + sector_index;
        read(lba, &mut buffer[..block]).map_err(|_| GptError::ReadFailed)?;
        let take = remaining.min(block);
        table_crc_state = crc32_continue(table_crc_state, &buffer[..take]);
        // 128 divides both supported sector sizes exactly, and table_bytes
        // is a multiple of 128, so entries cannot straddle sectors.
        for offset in (0..take).step_by(PARTITION_ENTRY_SIZE) {
            let entry = &buffer[offset..offset + PARTITION_ENTRY_SIZE];
            if entry[..16] == [0u8; 16] {
                if entry.iter().any(|b| *b != 0) {
                    return Err(GptError::InvalidPartition);
                }
                continue;
            }
            let first = le64(entry, 32);
            let last = le64(entry, 40);
            if first < first_usable || last > last_usable || first > last {
                return Err(GptError::InvalidPartition);
            }
            let mut unique_guid = [0u8; 16];
            unique_guid.copy_from_slice(&entry[16..32]);
            if unique_guid == [0; 16] {
                return Err(GptError::InvalidPartition);
            }
            for previous in info.iter() {
                if first <= previous.last_lba && last >= previous.first_lba
                    || previous.unique_guid == unique_guid
                {
                    return Err(GptError::PartitionOverlap);
                }
            }
            if info.partition_count == MAX_PARTITIONS {
                return Err(GptError::TooManyPartitions);
            }
            let mut type_guid = [0u8; 16];
            type_guid.copy_from_slice(&entry[..16]);
            info.partitions[info.partition_count] = Some(Partition {
                type_guid, unique_guid, first_lba: first, last_lba: last,
            });
            info.partition_count += 1;
        }
        remaining -= take;
    }
    if !table_crc_state != expected_table_crc {
        return Err(GptError::ArrayCrcMismatch);
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;

    const TOTAL: u64 = 256;

    fn put32(bytes: &mut [u8], pos: usize, number: u32) {
        bytes[pos..pos + 4].copy_from_slice(&number.to_le_bytes());
    }

    fn put64(bytes: &mut [u8], pos: usize, number: u64) {
        bytes[pos..pos + 8].copy_from_slice(&number.to_le_bytes());
    }

    fn rechecksum(disk: &mut [u8], block: usize) {
        let entries_bytes = 128 * 128;
        let start = block * 2;
        let table_crc = crc32(&disk[start..start + entries_bytes]);
        let h = &mut disk[block..block * 2];
        put32(h, 88, table_crc);
        put32(h, 16, 0);
        let header_crc = crc32(&h[..92]);
        put32(h, 16, header_crc);
    }

    fn sample_gpt(block: usize) -> std::vec::Vec<u8> {
        let mut image = vec![0u8; block * TOTAL as usize];
        image[510] = 0x55;
        image[511] = 0xaa;
        image[446 + 4] = 0xee;
        put32(&mut image, 446 + 8, 1);
        put32(&mut image, 446 + 12, (TOTAL - 1) as u32);
        let table_sectors = (128 * 128) / block;
        let first_usable = 2 + table_sectors as u64;
        let last_usable = TOTAL - table_sectors as u64 - 2;

        // Synthetic EFI + root partitions and unique GUIDs.
        let first_entry = block * 2;
        for (index, first, last) in [
            (0usize, first_usable, first_usable + 19),
            (1usize, first_usable + 20, first_usable + 65),
        ] {
            let base = first_entry + index * 128;
            image[base..base + 16].fill(index as u8 + 1);
            image[base + 16..base + 32].fill(index as u8 + 11);
            put64(&mut image, base + 32, first);
            put64(&mut image, base + 40, last);
        }

        let h = &mut image[block..block * 2];
        h[..8].copy_from_slice(b"EFI PART");
        put32(h, 8, 0x0001_0000);
        put32(h, 12, 92);
        put64(h, 24, 1);
        put64(h, 32, TOTAL - 1);
        put64(h, 40, first_usable);
        put64(h, 48, last_usable);
        h[56..72].fill(0x42);
        put64(h, 72, 2);
        put32(h, 80, 128);
        put32(h, 84, 128);
        rechecksum(&mut image, block);
        image
    }

    fn inspect(image: &[u8], block: usize) -> Result<GptInfo, GptError> {
        inspect_primary_gpt(block as u32, TOTAL, |lba, out| {
            let start = lba as usize * block;
            let source = image.get(start..start + block).ok_or(())?;
            out.copy_from_slice(source);
            Ok(())
        })
    }

    #[test]
    fn reads_valid_gpt_on_512_and_4096_byte_sectors() {
        for block in [512, 4096] {
            let image = sample_gpt(block);
            let info = inspect(&image, block).unwrap();
            assert_eq!(info.partition_count, 2);
            let entries: std::vec::Vec<_> = info.iter().collect();
            assert!(entries[0].last_lba < entries[1].first_lba);
            assert_eq!(info.backup_header_lba, TOTAL - 1);
        }
    }

    #[test]
    fn rejects_missing_mbr_hybrid_and_gpt_signature() {
        let mut image = sample_gpt(512);
        image[510] = 0;
        assert_eq!(inspect(&image, 512), Err(GptError::InvalidProtectiveMbr));
        image = sample_gpt(512);
        image[462 + 4] = 0x07;
        assert_eq!(inspect(&image, 512), Err(GptError::HybridMbr));
        image = sample_gpt(512);
        image[512] = b'X';
        assert_eq!(inspect(&image, 512), Err(GptError::BadSignature));
    }

    #[test]
    fn protective_mbr_must_cover_the_disk_and_reject_stale_slots() {
        let mut image = sample_gpt(512);
        put32(&mut image, 446 + 12, 7);
        assert_eq!(inspect(&image, 512), Err(GptError::InvalidProtectiveMbr));
        image = sample_gpt(512);
        image[462 + 1] = 1;
        assert_eq!(inspect(&image, 512), Err(GptError::HybridMbr));
    }

    #[test]
    fn nonzero_unused_entry_and_header_padding_are_rejected() {
        let mut image = sample_gpt(512);
        image[1024 + 2 * 128 + 20] = 1;
        rechecksum(&mut image, 512);
        assert_eq!(inspect(&image, 512), Err(GptError::InvalidPartition));
        image = sample_gpt(512);
        image[512 + 93] = 1;
        assert_eq!(inspect(&image, 512), Err(GptError::BadHeaderSize));
    }

    #[test]
    fn crc32_reference_and_header_corruption_are_detected() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        let mut image = sample_gpt(512);
        image[512 + 40] ^= 1;
        assert_eq!(inspect(&image, 512), Err(GptError::HeaderCrcMismatch));
    }

    #[test]
    fn entry_corruption_is_detected_even_when_metadata_is_in_bounds() {
        let mut image = sample_gpt(512);
        image[1024 + 30] ^= 1;
        assert_eq!(inspect(&image, 512), Err(GptError::ArrayCrcMismatch));
    }

    #[test]
    fn overlapping_partitions_fail_even_with_valid_crcs() {
        let mut image = sample_gpt(512);
        let first_start = le64(&image, 1024 + 32);
        put64(&mut image, 1024 + 128 + 32, first_start + 4);
        rechecksum(&mut image, 512);
        assert_eq!(inspect(&image, 512), Err(GptError::PartitionOverlap));
    }

    #[test]
    fn out_of_range_partition_is_rejected() {
        let mut image = sample_gpt(512);
        put64(&mut image, 1024 + 128 + 40, TOTAL);
        rechecksum(&mut image, 512);
        assert_eq!(inspect(&image, 512), Err(GptError::InvalidPartition));
    }

    #[test]
    fn malicious_partition_array_size_rejected_before_reading_it() {
        let mut image = sample_gpt(512);
        put32(&mut image, 512 + 80, u32::MAX);
        rechecksum(&mut image, 512);
        assert_eq!(inspect(&image, 512), Err(GptError::UnsupportedEntryLayout));
    }

    #[test]
    fn callback_failure_and_bad_geometry_are_fail_closed() {
        assert_eq!(inspect_primary_gpt(520, TOTAL, |_, _| Ok(())),
                   Err(GptError::BadGeometry));
        assert_eq!(inspect_primary_gpt(512, TOTAL, |_, _| Err(())),
                   Err(GptError::ReadFailed));
    }
}
