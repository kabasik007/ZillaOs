#Requires -Version 5.1
<#
.SYNOPSIS
  Safely prune retired ZillaOS I-M1 remote branches once they are fully
  reachable from origin/main. Dry-run by default; -Execute is required
  to perform destructive remote branch deletion.
.EXAMPLE
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\cleanup-i-m1-branches.ps1
.EXAMPLE
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\cleanup-i-m1-branches.ps1 -Execute
#>
[CmdletBinding()]
param([switch]$Execute)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$integrationHead = 'abc0d09c2adcd7df48adc1202604eb2d6b9f1f99'
$branches = @(
    @{ Name = 'agent/process-scheduler-r0-20261107'; Sha = '2279405b7f695771e07ac9bca89cf6ff957580f8' },
    @{ Name = 'docs/os-services-amd-foundation-20261107'; Sha = '36fc690dd163bb88791b30b4e7bce94757c1a100' },
    @{ Name = 'feat/amd-gpu-discovery'; Sha = 'ef35ecac312161f674dc7b04eb6d7989cef80908' },
    @{ Name = 'feat/ipc-capabilities-foundation'; Sha = 'ad9ad72d3abeae16ab6ad64490db2063e4a2cc7a' },
    @{ Name = 'feat/platform-im1-single-inventory'; Sha = 'be082dd9582b3bc3a3f40ebe4d76407896ebc781' },
    @{ Name = 'feat/platform-pci-foundation'; Sha = '4937ab131a3ee515756ae92d057f53781eccbfd1' },
    @{ Name = 'feat/service-manager-foundation'; Sha = '20895ecf372b4020a696aca2b9a3029f507873b5' },
    @{ Name = 'feature/graphics-display-contracts-g0-g1-20261107'; Sha = '3d1c728b34cba35825adca7bf92f9da6676d3010' },
    @{ Name = 'fix/windows-wsl-path-build'; Sha = 'ea8d451f6e1830c88bea83fda4a0446ae5535014' },
    @{ Name = 'integration/20261107-cross-workstreams'; Sha = 'abc0d09c2adcd7df48adc1202604eb2d6b9f1f99' }
)

Push-Location $repoRoot
try {
    & git rev-parse --is-inside-work-tree | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Not a Git repository: $repoRoot" }

    $origin = (& git remote get-url origin).Trim()
    if ($LASTEXITCODE -ne 0 -or $origin -notmatch '(?i)github\.com[:/]kabasik007/ZillaOs(?:\.git)?$') {
        throw "Refusing cleanup: origin is not the expected kabasik007/ZillaOs repository: $origin"
    }

    Write-Host "[ZillaOS] Refreshing origin/main without modifying local working files..."
    & git fetch origin --prune
    if ($LASTEXITCODE -ne 0) { throw 'git fetch failed' }

    $main = (& git rev-parse --verify refs/remotes/origin/main).Trim()
    if ($LASTEXITCODE -ne 0 -or $main -notmatch '^[0-9a-f]{40}$') {
        throw 'Cannot verify origin/main'
    }
    & git merge-base --is-ancestor $integrationHead $main
    if ($LASTEXITCODE -ne 0) {
        throw "Integration I-M1 is not yet in origin/main; refusing to delete branches."
    }

    $remoteLines = @(& git ls-remote --heads origin)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot list remote heads' }
    $remoteHeads = @{}
    foreach ($line in $remoteLines) {
        if ($line -match '^([0-9a-fA-F]{40})\s+refs/heads/(.+)$') {
            $remoteHeads[$Matches[2]] = $Matches[1].ToLowerInvariant()
        }
    }

    $eligible = 0
    $deleted = 0
    foreach ($item in $branches) {
        $name = [string]$item.Name
        $expected = [string]$item.Sha
        if (-not $remoteHeads.ContainsKey($name)) {
            Write-Host "[SKIP] Already absent: $name"
            continue
        }
        $actual = $remoteHeads[$name]
        if ($actual -ne $expected) {
            Write-Warning "[SKIP] Branch advanced since audit: $name ($actual)"
            continue
        }
        & git merge-base --is-ancestor $actual $main
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "[SKIP] Branch is NOT an ancestor of main: $name"
            continue
        }
        $eligible += 1
        if ($Execute) {
            & git push origin --delete $name
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "[FAILED] Git refused to delete: $name"
            } else {
                Write-Host "[DELETED] $name"
                $deleted += 1
            }
        } else {
            Write-Host "[DRY RUN] Eligible for deletion: $name"
        }
    }

    Write-Host "Eligible: $eligible. Deleted: $deleted. Main: $main"
    if (-not $Execute) {
        Write-Host 'Nothing deleted. Re-run with -Execute to prune only these verified legacy branches.'
    }
} finally {
    Pop-Location
}
