#requires -Version 5.1
# Helpers have no side effects when dot-sourced.
# Windows core.autocrlf may check out Bash scripts as CRLF.
function Get-ZillaUnixTextFiles([string]$Root) {
    $scriptDir = Join-Path $Root "scripts"
    if (Test-Path -LiteralPath $scriptDir -PathType Container) {
        Get-ChildItem -LiteralPath $scriptDir -Filter "*.sh" -File -Recurse
    }
    $bootConfig = Join-Path $Root "boot\limine.conf"
    if (Test-Path -LiteralPath $bootConfig -PathType Leaf) {
        Get-Item -LiteralPath $bootConfig
    }
}

function Get-ZillaCrFiles([string]$Root) {
    foreach ($file in @(Get-ZillaUnixTextFiles $Root)) {
        $content = [IO.File]::ReadAllText($file.FullName, [Text.Encoding]::UTF8)
        if ($content.Contains("`r")) { $file.FullName }
    }
}

function Repair-ZillaStagedText([string]$StageRoot) {
    $count = 0
    foreach ($path in @(Get-ZillaCrFiles $StageRoot)) {
        $text = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8)
        # Touch ONLY the private staged copy, never the user's checkout.
        $normalized = $text.Replace("`r`n", "`n").Replace("`r", "`n")
        [IO.File]::WriteAllText($path, $normalized, (New-Object System.Text.UTF8Encoding($false)))
        $count++
    }
    if ($count -gt 0) {
        Write-Host "[ZillaOS] Normalized $count staged Bash/boot config file(s) to LF."
    }
}
