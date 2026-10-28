#requires -Version 5.1
<#
.SYNOPSIS
Create a NEW, isolated VMware UEFI VM from a prebuilt ZillaOS test RAW image.
.DESCRIPTION
Windows PowerShell launcher. WSL verifies GPT+FAT32 and built boot assets,
then converts the RAW to a new monolithic sparse VMDK under build/.
Creates a VMware .vmx file. No host disk or pre-existing VM is modified.
.PARAMETER ConfirmNewVM
Required explicit approval of creating a new VMDK and VM configuration.
.PARAMETER Launch
Open the newly created VM in VMware Workstation (does not power it on).
#>
[CmdletBinding()]
param(
    [switch]$ConfirmNewVM,
    [switch]$Launch,
    [string]$Distro = "Ubuntu",
    [string]$ImageName = "zillaos-uefi.img"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $ConfirmNewVM) {
    throw "Explicit confirmation required: -ConfirmNewVM. No files changed."
}
if ($ImageName -cnotmatch '^zillaos-[a-z0-9][a-z0-9-]*\.img$') {
    throw "Only zillaos-<name>.img inside Ubuntu Installer Lab is accepted."
}
if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
    throw "WSL Ubuntu is required to validate the image and export VMDK."
}
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$VmFolder = Join-Path $RepoRoot "build\vmware-installed\ZillaOS-Installed-Test"
$VmDisk = Join-Path $VmFolder "zillaos-installed.vmdk"
$VmConfig = Join-Path $VmFolder "ZillaOS-Installed-Test.vmx"
$SerialLog = Join-Path $VmFolder "zillaos-installed-com1.log"

# The exporter itself refuses to touch an existing VM directory. Checking
# again here makes failure behavior obvious to someone using PowerShell.
if (Test-Path -LiteralPath $VmFolder) {
    throw "Existing VMware test directory preserved: $VmFolder. Do not overwrite a VM."
}

$linuxRepo = (& wsl.exe -d $Distro -- wslpath -a $RepoRoot)
if ($LASTEXITCODE -ne 0 -or -not $linuxRepo) {
    throw "Could not map repository to Ubuntu. Ensure Windows drive is mounted in WSL."
}
$linuxRepo = ($linuxRepo | Select-Object -Last 1).Trim()
$escapedRepo = $linuxRepo.Replace("'", "'\''")
$escapedName = $ImageName.Replace("'", "'\''")
$command = "cd '$escapedRepo' && bash scripts/export_vmware_test.sh '$escapedName'"

Write-Host "[ZillaOS] Checking source RAW and exporting fresh VMware VMDK..." -ForegroundColor Cyan
& wsl.exe -d $Distro -- bash -lc $command
if ($LASTEXITCODE -ne 0) {
    throw "VMDK export failed. Existing files were not intentionally replaced."
}
if (-not (Test-Path -LiteralPath $VmDisk -PathType Leaf)) {
    throw "Exporter finished but VMDK not found: $VmDisk"
}
if (Test-Path -LiteralPath $VmConfig) {
    throw "Refusing existing VMX: $VmConfig"
}

# A dedicated guest: only the newly created disposable VMDK is attached.
# There is no CD-ROM, network, shared folder or USB host passthrough.
$vmx = @'
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "17"
displayName = "ZillaOS Installed Disk (Development)"
guestOS = "other-64"
firmware = "efi"
uefi.secureBoot.enabled = "FALSE"
memsize = "1024"
numvcpus = "1"
cpuid.coresPerSocket = "1"
mks.enable3d = "FALSE"
svga.present = "TRUE"
svga.autodetect = "TRUE"
sata0.present = "TRUE"
sata0:0.present = "TRUE"
sata0:0.fileName = "@@DISK@@"
scsi0.present = "FALSE"
nvme0.present = "FALSE"
ide0.present = "FALSE"
ide1.present = "FALSE"
ethernet0.present = "FALSE"
usb.present = "FALSE"
ehci.present = "FALSE"
mouse.vusb.enable = "FALSE"
keyboard.vusb.enable = "FALSE"
vmmouse.present = "FALSE"
sharedFolder.maxNum = "0"
isolation.tools.copy.disable = "TRUE"
isolation.tools.paste.disable = "TRUE"
isolation.tools.dnd.disable = "TRUE"
serial0.present = "TRUE"
serial0.fileType = "file"
serial0.startConnected = "TRUE"
serial0.fileName = "@@SERIAL@@"
vmci0.present = "FALSE"
floppy0.present = "FALSE"
logging = "TRUE"
tools.syncTime = "FALSE"
'@
$vmx = $vmx.Replace("@@DISK@@", $VmDisk).Replace("@@SERIAL@@", $SerialLog)
[System.IO.File]::WriteAllText(
    $VmConfig, $vmx + [Environment]::NewLine,
    (New-Object System.Text.UTF8Encoding($false))
)

Write-Host ""
Write-Host "[ZillaOS] Isolated VMware test VM created." -ForegroundColor Green
Write-Host "VMX:          $VmConfig"
Write-Host "VMDK:         $VmDisk"
Write-Host "COM1 log:     $SerialLog"
Write-Host "Firmware:     UEFI, Secure Boot disabled"
Write-Host "Virtual disk: disposable clone of validated RAW; no CD-ROM"
Write-Host "Isolation:    network, shared folders and USB disabled"
Write-Host "NOTE: Boot and desktop behavior have NOT been verified in VMware."
Write-Host ""

if ($Launch) {
    $found = Get-Command vmware.exe -ErrorAction SilentlyContinue
    $exe = if ($found) { $found.Source } else { $null }
    if (-not $exe) {
        foreach ($candidate in @(
            (Join-Path $env:ProgramFiles "VMware\VMware Workstation\vmware.exe"),
            (Join-Path $env:ProgramFiles "Broadcom\VMware Workstation\vmware.exe")
        )) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                $exe = $candidate
                break
            }
        }
    }
    if ($exe) {
        Start-Process -FilePath $exe -ArgumentList ('"' + $VmConfig + '"')
    } else {
        Write-Host "Open the generated VMX in VMware Workstation:"
        Write-Host $VmConfig
    }
} else {
    Write-Host "Open the VMX in VMware Workstation, then manually power on."
}
