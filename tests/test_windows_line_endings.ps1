#requires -Version 5.1
# Safe test; no WSL, Rust or VMware required.
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "..\scripts\windows_line_endings.ps1")

$root = Join-Path ([IO.Path]::GetTempPath()) ("zillaos-lf-test-'space " + [guid]::NewGuid().ToString("N"))
$src = Join-Path $root "original with spaces"
$stage = Join-Path $root "staged copy"
try {
    New-Item -ItemType Directory -Path (Join-Path $src "scripts"), (Join-Path $src "boot") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $stage "scripts"), (Join-Path $stage "boot") -Force | Out-Null
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $shell = Join-Path $src "scripts\hello world.sh"
    $lfShell = Join-Path $src "scripts\already-lf.sh"
    $boot = Join-Path $src "boot\limine.conf"
    [IO.File]::WriteAllText($shell, "#!/bin/bash`r`nset -Eeuo pipefail`r`necho OK`r`n", $utf8)
    [IO.File]::WriteAllText($lfShell, "#!/bin/bash`necho LF`n", $utf8)
    [IO.File]::WriteAllText($boot, "timeout: 0`r`ndefault_entry: 1`r`n", $utf8)
    Copy-Item -LiteralPath $shell, $lfShell -Destination (Join-Path $stage "scripts")
    Copy-Item -LiteralPath $boot -Destination (Join-Path $stage "boot")
    $sourceHash = (Get-FileHash -LiteralPath $shell -Algorithm SHA256).Hash
    $lfHash = (Get-FileHash -LiteralPath $lfShell -Algorithm SHA256).Hash
    $before = @(Get-ZillaCrFiles $stage)
    if ($before.Count -ne 2) { throw "Expected two staged CRLF files, found $($before.Count)." }
    Repair-ZillaStagedText $stage
    $after = @(Get-ZillaCrFiles $stage)
    if ($after.Count -ne 0) { throw "Staged files still have CR after repair: $after" }
    if ((Get-FileHash -LiteralPath $shell -Algorithm SHA256).Hash -ne $sourceHash) {
        throw "Original checkout was modified"
    }
    if ((Get-FileHash -LiteralPath $lfShell -Algorithm SHA256).Hash -ne $lfHash) {
        throw "Already-LF source was modified"
    }
    $stagedShell = [IO.File]::ReadAllText((Join-Path $stage "scripts\hello world.sh"))
    if (-not $stagedShell.Contains("set -Eeuo pipefail`n")) {
        throw "Expected working Bash statement with LF after repair"
    }
    Write-Host "PASS: CRLF recognized, staged Bash and Limine normalized, original untouched."
} finally {
    if (Test-Path -LiteralPath $root) {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
