#requires -Version 5.1
<#
.SYNOPSIS
Windows build and VMware launch for ZillaOS UEFI desktop preview.
.DESCRIPTION
Uses WSL --exec with positional arguments, not a second implicit shell.
Unmounted or network-backed drives are staged in a Windows TEMP folder
that WSL can access. Setup installs packages only with -Action Setup.
#>
[CmdletBinding()]
param(
    [ValidateSet("Check", "Doctor", "Setup", "Build", "BuildRun", "Run", "OpenISO", "Smoke")]
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

function Invoke-WslRaw([string[]]$Arguments, [string]$Step) {
    Write-Host "[ZillaOS] $Step" -ForegroundColor Cyan
    # Windows PowerShell 5.1 can turn native stderr into a terminating
    # NativeCommandError. Display output as-is and inspect the exit code.
    $prior = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        & wsl.exe -d $Distro --exec @Arguments
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prior
    }
    if ($exitCode -ne 0) {
        throw "$Step failed in WSL (exit code $exitCode). See output above; ISO/VM launch aborted."
    }
}

function Assert-Wsl {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
        throw "WSL missing. Run Administrator PowerShell: wsl --install -d Ubuntu; reboot and finish Ubuntu setup."
    }
    Invoke-WslRaw @("/bin/sh", "-c", "true") "Checking Linux distribution '$Distro'"
    Invoke-WslRaw @("/bin/sh", "-c", "grep -qi wsl2 /proc/sys/kernel/osrelease") "Checking WSL2. Inspect: wsl --list --verbose; fix: wsl --set-version $Distro 2"
}

function Get-WslDirectory([string]$WindowsDirectory) {
    # wslpath receives the original Windows path as ONE argv element.
    # With --exec, backslashes and spaces are not touched by a shell.
    $prior = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $lines = @(& wsl.exe -d $Distro --exec /usr/bin/wslpath -u -a $WindowsDirectory 2>$null)
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prior
    }
    if ($exitCode -ne 0 -or $lines.Count -eq 0) { return $null }
    $lastLine = $lines | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Last 1
    if (-not $lastLine) { return $null }
    $linux = ([string]$lastLine).Trim()
    if (-not $linux.StartsWith("/")) { return $null }
    # Some drive letters can be translated but are not actually mounted.
    $entry = $linux.TrimEnd("/") + "/scripts/windows_wsl_entry.sh"
    $prior = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        & wsl.exe -d $Distro --exec /usr/bin/test -f $entry
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prior
    }
    if ($exitCode -ne 0) { return $null }
    return $linux
}

function New-StagedWorkspace {
    if (-not (Get-Command robocopy.exe -ErrorAction SilentlyContinue)) {
        throw "Repository drive unavailable in WSL and robocopy.exe is missing. Use a locally mounted directory."
    }
    if ([string]::IsNullOrWhiteSpace($env:TEMP)) {
        throw "Repository drive unavailable in WSL and TEMP is not configured."
    }
    $stage = Join-Path $env:TEMP ("ZillaOS-WSL-Stage-" + $PID + "-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Write-Host "[ZillaOS] Repo drive not mounted in WSL. Copying sources into $stage" -ForegroundColor Yellow
    # Never /MIR or /PURGE: no source files, VM, or user files are removed.
    $excluded = @(".git", "build", "dist", "target") | ForEach-Object { Join-Path $RepoRoot $_ }
    $copyArgs = @($RepoRoot, $stage, "/E", "/R:1", "/W:1", "/XD") + $excluded + @("/NFL", "/NDL", "/NJH", "/NJS", "/NP")
    $prior = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        & robocopy.exe @copyArgs | Out-Host
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prior
    }
    if ($code -ge 8) {
        throw "Source staging failed (robocopy exit $code). No original files changed. Stage: $stage"
    }
    $linuxStage = Get-WslDirectory $stage
    if (-not $linuxStage) {
        throw "Neither '$RepoRoot' nor TEMP '$stage' is readable in WSL '$Distro'. Check /etc/wsl.conf automount, run wsl --shutdown, or clone under Ubuntu. No original files were changed."
    }
    Write-Host "[ZillaOS] WSL stage: $linuxStage" -ForegroundColor Cyan
    return [pscustomobject]@{ WindowsPath = $stage; LinuxPath = $linuxStage; Staged = $true }
}

function Get-BuildWorkspace {
    $direct = Get-WslDirectory $RepoRoot
    if ($direct) {
        Write-Host "[ZillaOS] WSL repo: $direct"
        return [pscustomobject]@{ WindowsPath = $RepoRoot; LinuxPath = $direct; Staged = $false }
    }
    return New-StagedWorkspace
}

function Invoke-RepoAction($Workspace, [string]$Mode, [string]$Step) {
    $entry = $Workspace.LinuxPath.TrimEnd("/") + "/scripts/windows_wsl_entry.sh"
    $arguments = @("/bin/bash", $entry, $Workspace.LinuxPath, $Mode)
    if ($WithQemu -and $Mode -in @("check", "setup")) {
        $arguments += "--with-qemu"
    }
    Invoke-WslRaw $arguments $Step
}

function Copy-BuildLog($Workspace) {
    if (-not $Workspace.Staged) { return }
    $source = Join-Path $Workspace.WindowsPath "build\logs\windows-iso-build.log"
    if (Test-Path -LiteralPath $source -PathType Leaf) {
        $logs = Join-Path $RepoRoot "build\logs"
        New-Item -ItemType Directory -Force -Path $logs | Out-Null
        Copy-Item -LiteralPath $source -Destination (Join-Path $logs "windows-iso-build.log") -Force
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
    $workspace = Get-BuildWorkspace
    try {
        Invoke-RepoAction $workspace "build" "Testing, compiling Rust and generating UEFI ISO"
    } finally {
        Copy-BuildLog $workspace
    }

    if ($workspace.Staged) {
        $stageDist = Join-Path $workspace.WindowsPath "dist"
        New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
        foreach ($file in @("zillaos-0.0.2-dev-demo-uefi.iso", "zillaos-0.0.2-dev-demo-kernel.elf")) {
            $source = Join-Path $stageDist $file
            if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
                throw "Successful WSL build did not produce expected artifact: $source"
            }
            Copy-Item -LiteralPath $source -Destination (Join-Path $DistDir $file) -Force
        }
    }
    Assert-Iso

    # Refresh only this generated alias; never touch VMDKs or VM data.
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
    Write-Host "Experimental desktop/Setup preview, not a disk installer." -ForegroundColor Yellow
}

function Open-Vmware {
    Assert-Iso
    & (Join-Path $PSScriptRoot "vmware.ps1") -Launch -Distro $Distro -IsoPath $OriginalIso
    if (-not $?) { throw "VMware ISO-only launcher reported a failure." }
}

# Launching an already-built ISO requires neither WSL nor Ubuntu.
if ($Action -notin @("Run", "OpenISO")) { Assert-Wsl }
switch ($Action) {
    { $_ -in @("Check", "Doctor") } {
        $workspace = Get-BuildWorkspace
        Invoke-RepoAction $workspace "check" "Checking Ubuntu/Rust/xorriso prerequisites"
        Write-Host "Ready to build: .\scripts\windows.ps1 -Action Build"
        break
    }
    "Setup" {
        $workspace = Get-BuildWorkspace
        Invoke-RepoAction $workspace "setup" "Installing Ubuntu dependencies and pinned Rust (sudo may ask for password)"
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
        $workspace = Get-BuildWorkspace
        if ($workspace.Staged -and (Test-Path -LiteralPath $DistDir -PathType Container)) {
            Copy-Item -LiteralPath $DistDir -Destination (Join-Path $workspace.WindowsPath "dist") -Recurse -Force
        }
        Invoke-RepoAction $workspace "smoke" "Running graphical ISO boot smoke under QEMU/OVMF"
        break
    }
}
