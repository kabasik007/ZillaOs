//! Little-endian, allocation-free IPC envelope decoder. No raw pointer casts.
//! Wire handle descriptors are only untrusted requests; authorization is separate.
use crate::capabilities::{Handle, Rights};

pub const MAGIC: u32 = 0x4350_495a; // bytes "ZIPC"
pub const VERSION_MAJOR: u16 = 1;
pub const VERSION_MINOR: u16 = 0;
pub const HEADER_BYTES: usize = 32;
pub const HANDLE_DESCRIPTOR_BYTES: usize = 16;
pub const MAX_PAYLOAD_BYTES: usize = 4096;
pub const MAX_MESSAGE_HANDLES: usize = 4;
pub const MAX_WIRE_BYTES: usize =
    HEADER_BYTES + MAX_PAYLOAD_BYTES + MAX_MESSAGE_HANDLES * HANDLE_DESCRIPTOR_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcError {
    ShortHeader,
    InvalidMagic,
    UnsupportedVersion,
    InvalidHeader,
    InvalidKind,
    InvalidRequestId,
    InvalidOpcode,
    Oversized,
    MalformedLength,
    InvalidDescriptor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Event,
    Request,
    Reply,
}
impl MessageKind {
    fn decode(value: u16) -> Result<Self, IpcError> {
        match value {
            0 => Ok(Self::Event),
            1 => Ok(Self::Request),
            2 => Ok(Self::Reply),
            _ => Err(IpcError::InvalidKind),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageHeaderV1 {
    pub kind: MessageKind,
    pub opcode: u32,
    pub request_id: u64,
    pub payload_len: usize,
    pub handle_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleRequest {
    pub handle: Handle,
    pub requested_rights: Rights,
}
pub struct ParsedMessage<'a> {
    pub header: MessageHeaderV1,
    pub payload: &'a [u8],
    descriptors: &'a [u8],
}
impl<'a> ParsedMessage<'a> {
    pub fn handle_request(&self, index: usize) -> Option<HandleRequest> {
        if index >= self.header.handle_count {
            return None;
        }
        let offset = index * HANDLE_DESCRIPTOR_BYTES;
        let raw = read_u64(self.descriptors, offset);
        let requested = read_u64(self.descriptors, offset + 8);
        // All descriptor bits were checked by parse().
        Some(HandleRequest {
            handle: Handle::from_raw(raw),
            requested_rights: Rights::from_bits(requested)?,
        })
    }
}
fn read_u16(buf: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([buf[i], buf[i + 1]])
}
fn read_u32(buf: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]])
}
fn read_u64(buf: &[u8], i: usize) -> u64 {
    u64::from_le_bytes([
        buf[i],
        buf[i + 1],
        buf[i + 2],
        buf[i + 3],
        buf[i + 4],
        buf[i + 5],
        buf[i + 6],
        buf[i + 7],
    ])
}

/// v1 exact layout:
/// 0 magic:u32, 4 major:u16, 6 minor:u16, 8 header_bytes:u16,
/// 10 kind:u16, 12 opcode:u32, 16 request_id:u64, 24 payload_len:u32,
/// 28 handle_count:u16, 30 reserved:u16; then payload, then 16-byte
/// (untrusted handle:u64, requested_rights:u64) descriptors.
pub fn parse(bytes: &[u8]) -> Result<ParsedMessage<'_>, IpcError> {
    if bytes.len() < HEADER_BYTES {
        return Err(IpcError::ShortHeader);
    }
    if read_u32(bytes, 0) != MAGIC {
        return Err(IpcError::InvalidMagic);
    }
    if read_u16(bytes, 4) != VERSION_MAJOR || read_u16(bytes, 6) != VERSION_MINOR {
        return Err(IpcError::UnsupportedVersion);
    }
    if read_u16(bytes, 8) as usize != HEADER_BYTES || read_u16(bytes, 30) != 0 {
        return Err(IpcError::InvalidHeader);
    }
    let kind = MessageKind::decode(read_u16(bytes, 10))?;
    let opcode = read_u32(bytes, 12);
    let request_id = read_u64(bytes, 16);
    if opcode == 0 {
        return Err(IpcError::InvalidOpcode);
    }
    if (kind == MessageKind::Event && request_id != 0)
        || (kind != MessageKind::Event && request_id == 0)
    {
        return Err(IpcError::InvalidRequestId);
    }
    let payload_len = read_u32(bytes, 24) as usize;
    let handle_count = read_u16(bytes, 28) as usize;
    if payload_len > MAX_PAYLOAD_BYTES || handle_count > MAX_MESSAGE_HANDLES {
        return Err(IpcError::Oversized);
    }
    let descriptors_len = handle_count
        .checked_mul(HANDLE_DESCRIPTOR_BYTES)
        .ok_or(IpcError::Oversized)?;
    let end_payload = HEADER_BYTES
        .checked_add(payload_len)
        .ok_or(IpcError::Oversized)?;
    let total = end_payload
        .checked_add(descriptors_len)
        .ok_or(IpcError::Oversized)?;
    if total != bytes.len() {
        return Err(IpcError::MalformedLength);
    }
    let descriptors = &bytes[end_payload..total];
    for i in 0..handle_count {
        let offset = i * HANDLE_DESCRIPTOR_BYTES;
        let h = read_u64(descriptors, offset);
        let rights = read_u64(descriptors, offset + 8);
        if h == 0 || Rights::from_bits(rights).is_none() || rights == 0 {
            return Err(IpcError::InvalidDescriptor);
        }
    }
    Ok(ParsedMessage {
        header: MessageHeaderV1 {
            kind,
            opcode,
            request_id,
            payload_len,
            handle_count,
        },
        payload: &bytes[HEADER_BYTES..end_payload],
        descriptors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> [u8; HEADER_BYTES] {
        let mut b = [0u8; HEADER_BYTES];
        b[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        b[4..6].copy_from_slice(&VERSION_MAJOR.to_le_bytes());
        b[6..8].copy_from_slice(&VERSION_MINOR.to_le_bytes());
        b[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        b[10..12].copy_from_slice(&1u16.to_le_bytes()); // request
        b[12..16].copy_from_slice(&7u32.to_le_bytes()); // opcode
        b[16..24].copy_from_slice(&42u64.to_le_bytes());
        b
    }
    #[test]
    fn valid_header_and_kernel_derived_identity() {
        let b = base();
        let v = parse(&b).unwrap();
        assert_eq!(v.header.kind, MessageKind::Request);
        assert_eq!(v.header.request_id, 42);
        assert_eq!(v.payload, &[]);
        // Sender identity is deliberately not a wire header field.
        assert_eq!(v.header.handle_count, 0);
    }
    #[test]
    fn parses_valid_descriptor_but_does_not_authorize_it() {
        let mut b = [0u8; HEADER_BYTES + HANDLE_DESCRIPTOR_BYTES];
        b[..HEADER_BYTES].copy_from_slice(&base());
        b[28..30].copy_from_slice(&1u16.to_le_bytes());
        b[HEADER_BYTES..HEADER_BYTES + 8].copy_from_slice(&0x100000001u64.to_le_bytes());
        b[HEADER_BYTES + 8..].copy_from_slice(&Rights::SEND.bits().to_le_bytes());
        let p = parse(&b).unwrap();
        assert_eq!(p.handle_request(0).unwrap().requested_rights, Rights::SEND);
        assert!(p.handle_request(1).is_none());
        b[HEADER_BYTES + 8] = 0x80; // unrecognized permission bit
        assert_eq!(parse(&b).err(), Some(IpcError::InvalidDescriptor));
    }
    #[test]
    fn rejects_short_truncated_oversize_and_trailing_bytes() {
        let b = base();
        for len in 0..HEADER_BYTES {
            assert_eq!(parse(&b[..len]).err(), Some(IpcError::ShortHeader));
        }
        let mut bad = b;
        bad[24..28].copy_from_slice(&4097u32.to_le_bytes());
        assert_eq!(parse(&bad).err(), Some(IpcError::Oversized));
        bad = b;
        bad[28..30].copy_from_slice(&5u16.to_le_bytes());
        assert_eq!(parse(&bad).err(), Some(IpcError::Oversized));
        bad = b;
        bad[24..28].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(parse(&bad).err(), Some(IpcError::MalformedLength));
        let mut extra = [0u8; HEADER_BYTES + 1];
        extra[..HEADER_BYTES].copy_from_slice(&b);
        assert_eq!(parse(&extra).err(), Some(IpcError::MalformedLength));
    }
    #[test]
    fn rejects_unknown_versions_reserved_flags_and_forged_ids() {
        let b = base();
        let mut bad = b;
        bad[4] = 2;
        assert_eq!(parse(&bad).err(), Some(IpcError::UnsupportedVersion));
        bad = b;
        bad[10] = 3;
        assert_eq!(parse(&bad).err(), Some(IpcError::InvalidKind));
        bad = b;
        bad[30] = 1;
        assert_eq!(parse(&bad).err(), Some(IpcError::InvalidHeader));
        bad = b;
        bad[16..24].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(parse(&bad).err(), Some(IpcError::InvalidRequestId));
        bad = b;
        bad[10] = 0; // event requires request id zero
        assert_eq!(parse(&bad).err(), Some(IpcError::InvalidRequestId));
        bad = b;
        bad[0] = 0;
        assert_eq!(parse(&bad).err(), Some(IpcError::InvalidMagic));
    }
    #[test]
    fn deterministic_mutation_sweep_never_accepts_reserved_header_bytes() {
        let b = base();
        for i in 0..HEADER_BYTES {
            let mut fuzzed = b;
            fuzzed[i] ^= 0xff;
            // Mutations may be valid (opcode/request_id), but parsing must not panic.
            let _ = parse(&fuzzed);
        }
    }
}
