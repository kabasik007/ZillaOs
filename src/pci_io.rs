//! Minimal read-only PCI configuration mechanism #1 adapter for VMs.
//!
//! The OUT to 0xCF8 selects the PCI configuration DWORD; IN from 0xCFC
//! obtains the value. It does NOT issue writes to device configuration
//! registers, probe BAR sizes, enable bus-mastering or access disk sectors.
//! Only used by single-core 'vm-demo' in VMware/QEMU. Not a native-PC driver.

use core::arch::asm;
use zillaos_kernel::storage_pci::{PciLocation, StorageInventory, enumerate_storage};

#[inline]
fn read_config(loc: PciLocation, offset: u8) -> u32 {
    let address = loc.address(offset);
    let value: u32;
    // SAFETY: the VM runs ring 0 on x86-64 with permission to access
    // legacy PCI config ports. The first OUT changes only the CF8 address
    // selector and IN reads config data; no device config writes occur.
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") 0x0cf8u16,
            in("eax") address,
            options(nomem, nostack, preserves_flags)
        );
        asm!(
            "in eax, dx",
            in("dx") 0x0cfcu16,
            out("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Enumerate PCI functions for mass-storage controller class codes; all
/// contents of attached disks remain unknown, untouched and inaccessible.
pub fn inventory() -> StorageInventory {
    enumerate_storage(read_config)
}
