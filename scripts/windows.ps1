#requires -Version 5.1
<#
.SYNOPSIS
Windows build and launch entry point for the experimental ZillaOS UEFI ISO.
.DESCRIPTION
Builds the Rust kernel and Limine bootable ISO with Ubuntu/WSL2 + xorriso;
then optionally opens an isolated VMware ISO-only virtual machine.
Setup installs packages / Rust only by explicit -Action Setup.
No host physical disks, USB formatting or installed-disk VMDKs are touched.
.EXAMPLE
.\scripts\windows.ps1 -Action Doctor
.\scripts\windows.ps1 -Action Setup
.\scripts\windows.ps1 -Action Build
.\scripts\windows.ps1 -Action BuildRun
#>
[CmdletBinding()]
param(
    [ValidateSet("Doctor", "Setup", "Build", "BuildRun", "Run", "OpenISO", "Smoke")]
    [string]$Action = "Doctor",
    [string]$Distro = "Ubuntu",
    [switch]$WithQemu
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$DistDir = Join-Path $RepoRoot "dist"
$OriginalIso = Join-Path $DistDir "zillaos-0.0.2-dev-demo-uefi.iso"
$FriendlyIso = Join-Path $DistDir "ZillaOS-Desktop-UEFI.iso"

function Assert-Wsl {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
        throw "WSL2 missing. Open Administrator PowerShell: wsl --install -d Ubuntu; reboot and complete Ubuntu setup."
    }
    # Testing an actual Bash command avoids unreliable UTF-16/NUL output
    # from 'wsl --list --quiet' on some Windows builds.
    & wsl.exe -d $Distro -- bash -lc 'printf "ZILLAOS_WSL_READY\n"'
    if ($LASTEXITCODE -ne 0) {
        throw "Ubuntu WSL distro '$Distro' unavailable. Run: wsl --list --verbose"
    }
}

function Quote-Bash([string]$Value) {
    return "'" + $Value.Replace("'", "'\''") + "'"
}

function Get-LinuxRepo {
    $mapped = & wsl.exe -d $Distro -- wslpath -a $RepoRoot
    if ($LASTEXITCODE -ne 0 -or -not $mapped) {
        throw "Could not map the Windows repo to WSL: $RepoRoot. Check that the drive is mounted."
    }
    return ($mapped | Select-Object -Last 1).Trim()
}

function Invoke-Ubuntu([string]$BashCommand, [string]$Step) {
    Write-Host "[ZillaOS] $Step" -ForegroundColor Cyan
    & wsl.exe -d $Distro -- bash -lc $BashCommand
    $result = $LASTEXITCODE
    if ($result -ne 0) {
        throw "$Step failed in Ubuntu (exit code $result). Review the error above; ISO/VM launch was aborted."
    }
}

function Assert-Iso {
    if (-not (Test-Path -LiteralPath $OriginalIso -PathType Leaf)) {
        throw "No ZillaOS UEFI ISO: $OriginalIso. Choose Build first."
    }
    if ((Get-Item -LiteralPath $OriginalIso).Length -lt 65536) {
        throw "ISO is suspiciously small; refusing to use: $OriginalIso"
    }
}

function Build-Iso {
    $linuxRepo = Get-LinuxRepo
    $quotedRepo = Quote-Bash $linuxRepo
    $command = 'set -Eeuo pipefail; cd ' + $quotedRepo +
        '; if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi; ' +
        'mkdir -p build/logs; bash scripts/build_vmware.sh 2>&1 | tee build/logs/windows-iso-build.log'
    Invoke-Ubuntu $command "Compiling Rust kernel and generating UEFI ISO with Limine/xorriso"
    Assert-Iso

    # A human-friendly alias for Windows File Explorer. Only this generated
    # artifact is refreshed. No VMDK, physical disk, VMX or boot media writes.
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
    $temporary = Join-Path $DistDir ("ZillaOS-Desktop-UEFI.iso.tmp-" + $PID)
    try {
        Copy-Item -LiteralPath $OriginalIso -Destination $temporary -ErrorAction Stop
        Move-Item -LiteralPath $temporary -Destination $FriendlyIso -Force
    } finally {
        if (Test-Path -LiteralPath $temporary) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
    $sourceHash = (Get-FileHash -LiteralPath $OriginalIso -Algorithm SHA256).Hash
    $copyHash = (Get-FileHash -LiteralPath $FriendlyIso -Algorithm SHA256).Hash
    if ($sourceHash -ne $copyHash) {
        throw "ISO copy verification failed; do not distribute it."
    }
    $sizeMb = [Math]::Round((Get-Item -LiteralPath $FriendlyIso).Length / 1MB, 2)
    Write-Host ""
    Write-Host "UEFI ISO CREATED AND SHA256 VERIFIED" -ForegroundColor Green
    Write-Host "ISO:    $FriendlyIso"
    Write-Host "Size:   $sizeMb MB"
    Write-Host "SHA256: $copyHash"
    Write-Host "Build log: $(Join-Path $RepoRoot 'build\logs\windows-iso-build.log')"
    Write-Host "This ISO is a ZillaOS desktop/Setup preview, not a full disk installer." -ForegroundColor Yellow
}

function Open-Vmware {
    Assert-Iso
    # Reuse the repository's isolated UEFI VMware VM builder.
    # It attaches only a virtual CD-ROM with ISO: no writable hard disk.
    & (Join-Path $PSScriptRoot "vmware.ps1") -Launch -Distro $Distro -IsoPath $OriginalIso
    if (-not $?) {
        throw "VMware ISO-only launcher reported a failure."
    }
}

Assert-Wsl
switch ($Action) {
    "Doctor" {
        $linuxRepo = Get-LinuxRepo
        $command = 'cd ' + (Quote-Bash $linuxRepo) + '; bash scripts/windows_wsl_setup.sh --check'
        if ($WithQemu) { $command += ' --with-qemu' }
        Invoke-Ubuntu $command "Checking Rust/xorriso and Ubuntu prerequisites"
        Write-Host "Ready to build: .\scripts\windows.ps1 -Action Build"
        break
    }
    "Setup" {
        $linuxRepo = Get-LinuxRepo
        # Explicit Setup action authorizes package and rustup installation
        # inside this Ubuntu distribution. It never installs ZillaOS to disk.
        $command = 'cd ' + (Quote-Bash $linuxRepo) +
            '; bash scripts/windows_wsl_setup.sh --install --install-rust'
        if ($WithQemu) { $command += ' --with-qemu' }
        Invoke-Ubuntu $command "Installing Ubuntu packages and pinned Rust toolchain (sudo password may be requested)"
        break
    }
    "Build" { Build-Iso; break }
    "BuildRun" { Build-Iso; Open-Vmware; break }
    "Run" { Open-Vmware; break }
    "OpenISO" {
        if (-not (Test-Path -LiteralPath $DistDir -PathType Container)) {
            throw "No output folder yet. Build the ISO first."
        }
        Start-Process -FilePath explorer.exe -ArgumentList ('"' + $DistDir + '"')
        break
    }
    "Smoke" {
        $linuxRepo = Get-LinuxRepo
        $command = 'set -Eeuo pipefail; cd ' + (Quote-Bash $linuxRepo) +
            '; python3 scripts/desktop_smoke.py --timeout 120'
        Invoke-Ubuntu $command "Running graphical ISO boot smoke under QEMU/OVMF"
        break
    }
}
