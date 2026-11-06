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

 
/// Read-only PCI video identification; does not enable the SVGA FIFO,
/// map GPU BARs, change a mode or issue any accelerated commands.
pub fn video_adapter() -> zillaos_kernel::video::VideoAdapter {
    use zillaos_kernel::video::gpu::vmware_svga;

    let adapter = zillaos_kernel::video::probe_pci_video(read_config);
    // One read-only probe per desktop boot; failure does not touch GOP.
    #[cfg(feature = "svga-id-negotiate")]
    let probe_result = {
        crate::serial::write("ZILLAOS_SVGA_ID_NEGOTIATION=OPT_IN\n");
        vmware_svga::probe_with_negotiation(
            read_config,
            read_svga_register,
            write_svga_id,
        )
    };
    #[cfg(not(feature = "svga-id-negotiate"))]
    let probe_result = vmware_svga::probe(read_config, read_svga_register);
    match probe_result {
        Ok(Some(info)) => {
            serial_probe_hex("ZILLAOS_SVGA_ID_HEX=", info.registers.interface_id as u64);
            serial_probe_hex("ZILLAOS_SVGA_MAX_WIDTH_HEX=", info.registers.max_width as u64);
            serial_probe_hex("ZILLAOS_SVGA_MAX_HEIGHT_HEX=", info.registers.max_height as u64);
            serial_probe_hex("ZILLAOS_SVGA_VRAM_BYTES_HEX=", info.registers.vram_bytes as u64);
            serial_probe_hex("ZILLAOS_SVGA_FB_BYTES_HEX=", info.registers.framebuffer_bytes as u64);
            if let Some(bytes) = info.registers.fifo_bytes {
                serial_probe_hex("ZILLAOS_SVGA_FIFO_BYTES_HEX=", bytes as u64);
            }
            if let Some(caps) = info.registers.capability_bits {
                serial_probe_hex("ZILLAOS_SVGA_CAPABILITIES_HEX=", caps as u64);
            }
            crate::serial::write("ZILLAOS_SVGA_PROBE=READ_ONLY_OK\n");
        }
        Ok(None) => crate::serial::write("ZILLAOS_SVGA_PROBE=NO_DEVICE\n"),
        Err(error) => {
            crate::serial::write("ZILLAOS_SVGA_PROBE=FALLBACK_");
            crate::serial::write(error.label());
            crate::serial::write("\n");
        }
    }
    adapter
}
fn serial_probe_hex(name: &str, number: u64) {
    crate::serial::write(name);
    crate::serial::write_hex(number);
    crate::serial::write("\n");
}
/// Only SVGA INDEX I/O selector is written, SVGA VALUE is read-only.
/// SAFETY: probe validates PCI BAR0 type, port range, decoder enable and
/// device identity before this routine is called. Single-core VM boot owns
/// access to both ports; no concurrency or reentrant SVGA access exists.
/// No PCI config or SVGA register value writes, MMIO, DMA or FIFO commands.
fn read_svga_register(base: u16, index: u32) -> u32 {
    let result: u32;
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") base,
            in("eax") index,
            options(nomem, nostack, preserves_flags)
        );
        asm!(
            "in eax, dx",
            in("dx") (base + 1),
            out("eax") result,
            options(nomem, nostack, preserves_flags)
        );
    }
    result
}

/// The only SVGA device-value write in this experimental opt-in path.
/// SAFETY: only called after PCI identity, I/O decoder and BAR0 validation.
/// The index is allowlisted to REG_ID; the value must be a known interface
/// ID. The single-core VM boot serializes port access. Does not enable FIFO,
/// map MMIO, write PCI config space, or select a graphics mode.
#[cfg(feature = "svga-id-negotiate")]
fn write_svga_id(base: u16, index: u32, value: u32) {
    use zillaos_kernel::video::gpu::vmware_svga::registers;
    if index != registers::ID || !registers::known_id(value) {
        return;
    }
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") base,
            in("eax") registers::ID,
            options(nomem, nostack, preserves_flags)
        );
        asm!(
            "out dx, eax",
            in("dx") (base + 1),
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}
