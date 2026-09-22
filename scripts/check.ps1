<#
.SYNOPSIS
  Every gate that guards this repository, run locally.

.DESCRIPTION
  There is no CI while the repository is private, so this script is the only thing
  standing between a mistake and main. Each gate is checked on its own exit status
  and reported in a summary, rather than chained in a way that can report success
  without having checked anything.

.EXAMPLE
  .\scripts\check.ps1
  .\scripts\check.ps1 -Full
#>
param([switch]$Full)

$ErrorActionPreference = 'Continue'
Set-Location (Join-Path $PSScriptRoot '..')

$pass = @()
$fail = @()

function Invoke-Gate {
    param([string]$Name, [scriptblock]$Body)
    Write-Host ""
    Write-Host "== $Name ==" -ForegroundColor White
    & $Body
    if ($LASTEXITCODE -eq 0) { $script:pass += $Name } else { $script:fail += $Name }
}

Invoke-Gate 'fmt'    { cargo fmt --all --check }
Invoke-Gate 'clippy' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Gate 'test'   { cargo test --workspace --all-targets }
Invoke-Gate 'doc'    { cargo doc --workspace --no-deps }

# Identical inputs must produce identical results on any core count. Without this,
# golden-file comparisons are flaky and real regressions become invisible.
Write-Host ""
Write-Host "== determinism ==" -ForegroundColor White
$env:RAYON_NUM_THREADS = '1'; cargo test --workspace --quiet *> $null; $one = $LASTEXITCODE
$env:RAYON_NUM_THREADS = '8'; cargo test --workspace --quiet *> $null; $eight = $LASTEXITCODE
Remove-Item Env:\RAYON_NUM_THREADS -ErrorAction SilentlyContinue
if ($one -eq 0 -and $eight -eq 0) {
    $pass += 'determinism'; Write-Host 'tests agree at 1 and 8 threads'
} else {
    $fail += 'determinism'; Write-Host 'tests differ across thread counts'
}

if ($Full) { Invoke-Gate 'bench' { cargo bench --workspace --no-fail-fast } }

Write-Host ""
Write-Host "== summary ==" -ForegroundColor White
foreach ($p in $pass) { Write-Host "  pass  $p" -ForegroundColor Green }
foreach ($f in $fail) { Write-Host "  FAIL  $f" -ForegroundColor Red }

if ($fail.Count -gt 0) {
    Write-Host ""
    Write-Host "$($fail.Count) gate(s) failed."
    exit 1
}
Write-Host ""
Write-Host "All gates passed."
