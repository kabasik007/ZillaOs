//! Conservative x86-64 platform feature probe.
//! Boot is single-core; these diagnostics do not enable paging or NX.

use core::arch::x86_64::__cpuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuInfo {
    pub physical_address_bits: u8,
    pub virtual_address_bits: u8,
    pub nx_supported: bool,
    pub page_1g_supported: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuError {
    MissingExtendedFeatureLeaf,
    UnsupportedPhysicalAddressWidth,
    UnsupportedVirtualAddressWidth,
}

fn validated_address_bits(raw: u32) -> Result<(u8, u8), CpuError> {
    let physical = (raw & 0xff) as u8;
    let virtual_bits = ((raw >> 8) & 0xff) as u8;
    if !(32..=52).contains(&physical) {
        return Err(CpuError::UnsupportedPhysicalAddressWidth);
    }
    if virtual_bits != 48 && virtual_bits != 57 {
        return Err(CpuError::UnsupportedVirtualAddressWidth);
    }
    Ok((physical, virtual_bits))
}

pub fn detect() -> Result<CpuInfo, CpuError> {
    // SAFETY: CPUID is supported by x86-64 and has no privileged side effects.
    let extended = unsafe { __cpuid(0x8000_0000) };
    if extended.eax < 0x8000_0008 {
        return Err(CpuError::MissingExtendedFeatureLeaf);
    }
    // SAFETY: supported extended leaves checked above.
    let features = unsafe { __cpuid(0x8000_0001) };
    // SAFETY: supported extended leaves checked above.
    let address = unsafe { __cpuid(0x8000_0008) };
    let (physical_address_bits, virtual_address_bits) = validated_address_bits(address.eax)?;
    Ok(CpuInfo {
        physical_address_bits,
        virtual_address_bits,
        nx_supported: features.edx & (1 << 20) != 0,
        page_1g_supported: features.edx & (1 << 26) != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_48_bit_virtual_and_36_bit_physical() {
        assert_eq!(validated_address_bits((48 << 8) | 36).unwrap(), (36, 48));
    }

    #[test]
    fn recognizes_la57() {
        assert_eq!(validated_address_bits((57 << 8) | 52).unwrap(), (52, 57));
    }

    #[test]
    fn rejects_invalid_widths() {
        assert_eq!(
            validated_address_bits((48 << 8) | 20),
            Err(CpuError::UnsupportedPhysicalAddressWidth)
        );
        assert_eq!(
            validated_address_bits((42 << 8) | 36),
            Err(CpuError::UnsupportedVirtualAddressWidth)
        );
    }
}
