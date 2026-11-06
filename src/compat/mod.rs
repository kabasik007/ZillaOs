pub mod windows_pe;
//! Future multi-platform executable routing for ZillaOS.
//!
//! This module parses only minimal file signatures. It never executes code,
//! loads libraries, mounts archives, or claims binary compatibility.
//! Native processes, Linux ABI, Win32, Android and console emulation require
//! separate isolated backends. A header alone cannot identify the guest ABI.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutableFormat {
    Elf,
    PortableExecutable,
    ZipArchive,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetPlatform {
    ZillaNative,
    Linux,
    Windows,
    Android,
    PlayStation,
    Xbox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeStatus {
    NotImplemented,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeBackend {
    pub target: TargetPlatform,
    pub status: RuntimeStatus,
}

/// All future compatibility backends are explicit and disabled until there
/// is a process model, memory isolation, syscall layer and test coverage.
pub const BACKENDS: [RuntimeBackend; 6] = [
    RuntimeBackend { target: TargetPlatform::ZillaNative, status: RuntimeStatus::NotImplemented },
    RuntimeBackend { target: TargetPlatform::Linux, status: RuntimeStatus::NotImplemented },
    RuntimeBackend { target: TargetPlatform::Windows, status: RuntimeStatus::NotImplemented },
    RuntimeBackend { target: TargetPlatform::Android, status: RuntimeStatus::NotImplemented },
    RuntimeBackend { target: TargetPlatform::PlayStation, status: RuntimeStatus::NotImplemented },
    RuntimeBackend { target: TargetPlatform::Xbox, status: RuntimeStatus::NotImplemented },
];

/// A signature is not evidence that an executable is safe or compatible.
/// ZIP may be an Android package, a document, a theme or a random archive.
pub fn detect_format(bytes: &[u8]) -> ExecutableFormat {
    if bytes.len() >= 5 && bytes.starts_with(b"\x7fELF") {
        return ExecutableFormat::Elf;
    }
    if bytes.len() >= 0x40 && bytes.starts_with(b"MZ") {
        let pe_offset = u32::from_le_bytes([
            bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f],
        ]) as usize;
        if let Some(signature) = bytes.get(pe_offset..pe_offset.saturating_add(4)) {
            if signature == b"PE\0\0" {
                return ExecutableFormat::PortableExecutable;
            }
        }
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return ExecutableFormat::ZipArchive;
    }
    ExecutableFormat::Unknown
}

pub const fn backend_available(_target: TargetPlatform) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_formats_without_misidentifying_archives_as_android() {
        assert_eq!(detect_format(b"\x7fELF\x02"), ExecutableFormat::Elf);
        assert_eq!(detect_format(b"PK\x03\x04.apk data"),
                   ExecutableFormat::ZipArchive);
        assert_eq!(detect_format(b"MZ"), ExecutableFormat::Unknown);
        assert_eq!(detect_format(&[]), ExecutableFormat::Unknown);
        let mut pe = [0u8; 128];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x40;
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        assert_eq!(detect_format(&pe), ExecutableFormat::PortableExecutable);
        pe[0x40] = 0;
        assert_eq!(detect_format(&pe), ExecutableFormat::Unknown);
    }
    #[test]
    fn no_foreign_or_native_application_exec_backends_are_enabled() {
        for backend in BACKENDS {
            assert_eq!(backend.status, RuntimeStatus::NotImplemented);
            assert!(!backend_available(backend.target));
        }
    }
}
