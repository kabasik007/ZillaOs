# VMware Installed Disk — first no-ISO boot from FAT32/VMDK

**State: source committed; real boot NOT verified (2026-10-28).**
This tests a staged ZillaOS kernel loaded directly from a disposable
GPT/FAT32 EFI disk, rather than the live ISO. It is not an in-guest
installer. Root filesystem and settings persistence are still missing.

## Overview

~~~
Ubuntu Installer Lab new-only RAW (in WSL HOME)
  -> independent GPT + FAT32 + staged EFI-file validation
  -> qemu-img exports a NEW sparse VMDK
  -> qemu-img validates generated VMDK geometry
  -> Windows PowerShell generates a NEW isolated VMware .vmx
  -> VMware Workstation UEFI loads from VMDK, without ISO
~~~

The source RAW image is never modified by the converter. The exported
VMware clone is disposable and can be modified by the VM after startup;
do not attach a host physical disk, existing VM disk or shared folder.

## Step 1: prepare and verify RAW in WSL Ubuntu

~~~bash
cd /mnt/m/projects/ZillaOs
git pull origin main
sudo apt update
sudo apt install -y qemu-utils qemu-system-x86 ovmf xorriso
source "$HOME/.cargo/env"

python3 -m unittest discover -s tests -v
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build_vmware.sh

# Must use a name that does not exist already.
python3 scripts/installer_lab.py \
  --name zillaos-uefi.img --size-gib 12 --stage-uefi \
  --create --confirm CREATE-NEW-VM-IMAGE

python3 scripts/installer_fat32_verify.py \
  "$HOME/.local/share/zillaos/installer-lab/zillaos-uefi.img" --check-boot
~~~

This creates a NEW sparse 12-GiB RAW file in the Ubuntu filesystem;
it never overwrites an old image. Boot assets must be actual files
from a successful ZillaOS Rust/UEFI build.

## Step 2: test no-ISO boot in QEMU first

~~~bash
bash scripts/test_uefi_disk_qemu.sh zillaos-uefi.img
~~~

QEMU attaches RAW read-only with temporary OVMF NVRAM, no network,
no host drive passthrough and no ISO. The actual success gates are
ZILLAOS_BOOT_OK and ZILLAOS_SETUP_READY in COM1 serial plus a visible
GUI screen. This has not yet been demonstrated.

## Step 3: create separate VMware VMDK + VMX from Windows PowerShell

Exit Ubuntu and enter Windows PowerShell, NOT the WSL shell:

~~~powershell
cd M:\projects\ZillaOs
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\vmware_installed.ps1 -ConfirmNewVM -Launch
~~~

The script calls the WSL exporter after independent image validation.
It refuses to overwrite the VM folder or existing VMDK. It builds:

~~~
M:\projects\ZillaOs\build\vmware-installed\ZillaOS-Installed-Test\
    zillaos-installed.vmdk
    ZillaOS-Installed-Test.vmx
    disk-info.json
    zillaos-installed-com1.log       (appears when guest writes serial)
~~~

VMware config: UEFI, Secure Boot disabled, 1 CPU, 1024 MiB RAM,
software SVGA, only the new SATA VMDK, no CD/DVD, no network,
no shared folders or USB host passthrough. Launch just opens VMware
Workstation; you still manually select Power on.

If VMware does not open, use File > Open and select the VMX above.

## Diagnostics

In Windows PowerShell, after boot:

~~~powershell
Get-Content .\build\vmware-installed\ZillaOS-Installed-Test\zillaos-installed-com1.log -Tail 100
Get-Content .\build\vmware-installed\ZillaOS-Installed-Test\vmware.log -Tail 100
~~~

If COM1 is absent, the firmware may not have found BOOTX64.EFI or the
kernel may have failed before serial initialization. Use VMware's EFI
firmware menu to inspect the virtual disk and disable Secure Boot.

**Not yet claimed:** a successful VMware boot, working root filesystem,
persistent settings, install-to-disk UI or data-safe rollback. The
guest has no trusted block-device service or installer write path.

## Next release gates

1. Compile the Rust kernel and make QEMU no-ISO boot pass.
2. Confirm first boot from disposable VMDK in VMware.
3. Implement guest read-only block access, device IDs and root FS.
4. Persist language, computer name, theme and timezone across reboot.
5. Only then design an in-guest transactional installer with explicit
   target-disk confirmation, failure recovery and virtual-disk-only tests.

Related: [Installer V2.1](EFI_BOOT_DISK.md),
[Storage](STORAGE.md), [GPT](GPT.md), [VMware live ISO](VMWARE.md).
