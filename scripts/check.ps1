<#
.SYNOPSIS
  Every gate that guards this repository, run locally.

.DESCRIPTION
  There is no CI while the repository is private, so this script is the only thing
  standing between a mistake and main. Each gate is checked on its own exit status
  and reported in a summary, rather than chained in a way that can report success
  without having checked anything.

  Unlike scripts/check.sh, this always checks every crate.

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

# Identical inputs must produce identical results on any core count. The evidence
# is in tests that compare output against itself (see check.sh for why running the
# suite at two thread counts proved nothing). `--exact` passes on a name that
# matches nothing, so each is named by its whole path and the gate fails unless
# as many ran as are named.
$determinism = @(
    'run::tests::bounded_runs_are_deterministic'
    'cutter::dropcutter::tests::is_deterministic'
    'strategy::scallop::tests::is_deterministic'
    'strategy::waterline::tests::is_deterministic'
    'strategy::morph::tests::is_deterministic'
    'strategy::ramp::tests::is_deterministic'
    'path::ordering::tests::ordering_is_deterministic'
)
Invoke-Gate 'determinism' {
    $out = cargo test -p camcraft-core --lib --quiet -- --exact @determinism 2>&1
    $code = $LASTEXITCODE
    $out | ForEach-Object { Write-Host $_ }
    if ($code -eq 0 -and -not ($out -match "test result: ok\. $($determinism.Count) passed")) {
        Write-Host "expected $($determinism.Count) determinism tests to run: a name matches nothing" -ForegroundColor Red
        $code = 1
    }
    $global:LASTEXITCODE = $code
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
