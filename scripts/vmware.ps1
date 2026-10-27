#requires -Version 5.1
<#
.SYNOPSIS
Create an isolated VMware Workstation UEFI test VM for ZillaOS.
.DESCRIPTION
Runs on Windows PowerShell / PowerShell 7, not inside Ubuntu.
Optional -Build delegates the Rust/ISO build to WSL Ubuntu.
The resulting VM boots only a read-only ISO. No virtual/host disk,
no networking, no shared folders or USB HID passthrough are configured.
.PARAMETER Build
Build and validate demo ISO in the selected WSL distribution first.
.PARAMETER Launch
Open the generated VMX file in VMware Workstation.
.PARAMETER Force
Replace an existing VMX (only while the VM is powered off). Never
deletes firmware NVRAM, ISO, snapshots, files or VMDKs.
#>
[CmdletBinding()]
param(
    [switch]$Build,
    [switch]$Launch,
    [switch]$Force,
    [string]$Distro = "Ubuntu",
    [string]$IsoPath = "",
    [string]$VmDir = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $IsoPath) {
    $IsoPath = Join-Path $RepoRoot "dist\zillaos-0.0.2-dev-demo-uefi.iso"
}
if (-not $VmDir) {
    $VmDir = Join-Path $RepoRoot "build\vmware\ZillaOS"
}

if ($Build) {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
        throw "WSL is missing. Install Ubuntu under WSL2 first."
    }
    Write-Host "[ZillaOS] Resolving repository path in WSL $Distro ..."
    $linuxRepo = (& wsl.exe -d $Distro -- wslpath -a $RepoRoot)
    if ($LASTEXITCODE -ne 0 -or -not $linuxRepo) {
        throw "WSL cannot access $RepoRoot. Check the distribution and Windows drive mount."
    }
    $linuxRepo = ($linuxRepo | Select-Object -Last 1).Trim()
    # Pass a quoted POSIX path to bash, escaping embedded apostrophes.
    $safeLinuxRepo = $linuxRepo.Replace("'", "'\''")
    $buildCommand = "cd '$safeLinuxRepo' && bash scripts/build_vmware.sh"
    Write-Host "[ZillaOS] Building from WSL: $linuxRepo"
    & wsl.exe -d $Distro -- bash -lc $buildCommand
    if ($LASTEXITCODE -ne 0) {
        throw "WSL build failed. Fix the Rust/ISO build error shown above; no VMX was changed."
    }
}

if (-not (Test-Path -LiteralPath $IsoPath -PathType Leaf)) {
    throw "Demo ISO not found: $IsoPath. First run in Ubuntu: cd /mnt/m/projects/ZillaOs && bash scripts/build_vmware.sh"
}
$iso = (Resolve-Path -LiteralPath $IsoPath).Path
if ([IO.Path]::GetExtension($iso).ToLowerInvariant() -ne ".iso") {
    throw "Expected an .iso image, not: $iso"
}
if ((Get-Item -LiteralPath $iso).Length -lt 65536) {
    throw "ISO is suspiciously small; do not boot: $iso"
}
$vmFolder = [IO.Path]::GetFullPath($VmDir)
$vmxPath = Join-Path $vmFolder "ZillaOS.vmx"
if (Test-Path -LiteralPath $vmxPath) {
    if (-not $Force) {
        Write-Host "[ZillaOS] Existing VMX preserved (use -Force ONLY when powered off):" -ForegroundColor Yellow
        Write-Host $vmxPath
        Write-Host "[ZillaOS] ISO reference in VM settings should point to: $iso"
        if ($Launch) {
            # Launch existing VM configuration without editing it.
            $Vmware = Get-Command vmware.exe -ErrorAction SilentlyContinue
            $exe = if ($Vmware) { $Vmware.Source } else { $null }
            if (-not $exe) {
                foreach ($candidate in @(
                    (Join-Path $env:ProgramFiles "VMware\VMware Workstation\vmware.exe"),
                    (Join-Path $env:ProgramFiles "Broadcom\VMware Workstation\vmware.exe")
                )) {
                    if (Test-Path $candidate) { $exe = $candidate; break }
                }
            }
            if ($exe) { Start-Process -FilePath $exe -ArgumentList ('"' + $vmxPath + '"') }
            else { Write-Host "Open the VMX manually in VMware Workstation: $vmxPath" }
        }
        return
    }
    Write-Host "[ZillaOS] Replacing VMX configuration only; no other files are deleted." -ForegroundColor Yellow
}

New-Item -ItemType Directory -Force -Path $vmFolder | Out-Null
$serialPath = Join-Path $vmFolder "zillaos-com1.log"

# Keep VMX ASCII-only by using portable (ASCII) virtual device names.
# Windows native absolute paths can be used for ISO and serial file names.
$vmx = @'
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "17"
displayName = "ZillaOS Desktop (UEFI Development)"
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
sata0:0.deviceType = "cdrom-image"
sata0:0.startConnected = "TRUE"
sata0:0.fileName = "@@ISO@@"
scsi0.present = "FALSE"
nvme0.present = "FALSE"
ethernet0.present = "FALSE"
usb.present = "FALSE"
ehci.present = "FALSE"
mouse.vusb.enable = "FALSE"
keyboard.vusb.enable = "FALSE"
vmmouse.present = "FALSE"
serial0.present = "TRUE"
serial0.fileType = "file"
serial0.startConnected = "TRUE"
serial0.fileName = "@@SERIAL@@"
vmci0.present = "FALSE"
floppy0.present = "FALSE"
logging = "TRUE"
tools.syncTime = "FALSE"
'@
$vmx = $vmx.Replace("@@ISO@@", $iso).Replace("@@SERIAL@@", $serialPath)
[IO.File]::WriteAllText($vmxPath, $vmx + [Environment]::NewLine,
    (New-Object System.Text.UTF8Encoding($false)))

Write-Host ""
Write-Host "[ZillaOS] VMware development VM created." -ForegroundColor Green
Write-Host "VMX:        $vmxPath"
Write-Host "ISO:        $iso"
Write-Host "COM1 log:   $serialPath"
Write-Host "Firmware:   UEFI (Secure Boot disabled)"
Write-Host "Hardware:   1 CPU, 1024 MiB RAM, software SVGA"
Write-Host "Isolation:  No disk, network, USB passthrough or shared folders"
Write-Host "Input:      PS/2 emulation requested; VMware PS/2 support is not yet tested."
Write-Host ""
Write-Host "If VMware reports an unsupported virtual hardware version, create a custom"
Write-Host "UEFI VM manually and attach the same ISO as a virtual CD/DVD."
Write-Host ""

if ($Launch) {
    $found = Get-Command vmware.exe -ErrorAction SilentlyContinue
    $exe = if ($found) { $found.Source } else { $null }
    if (-not $exe) {
        foreach ($candidate in @(
            (Join-Path $env:ProgramFiles "VMware\VMware Workstation\vmware.exe"),
            (Join-Path $env:ProgramFiles "Broadcom\VMware Workstation\vmware.exe")
        )) {
            if (Test-Path $candidate) { $exe = $candidate; break }
        }
    }
    if ($exe) {
        Start-Process -FilePath $exe -ArgumentList ('"' + $vmxPath + '"')
    } else {
        Write-Host "VMware executable not located. Open the VMX manually:"
        Write-Host $vmxPath
    }
} else {
    Write-Host "Next: Open the VMX in VMware Workstation and click Power on."
}
