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
  .\scripts\check.ps1 -OpenCascade   also lint camcraft-import's `opencascade`
                                     feature and cross-check ogeom with it
                                     (builds OpenCascade: needs cmake, C++)
#>
param([switch]$Full, [switch]$OpenCascade)

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
# matches nothing: check the count it reports after renaming one.
Invoke-Gate 'determinism' {
    cargo test -p camcraft-core --quiet -- --exact `
        run::tests::bounded_runs_are_deterministic `
        dropcutter::tests::is_deterministic `
        scallop::tests::is_deterministic `
        waterline::tests::is_deterministic `
        morph::tests::is_deterministic `
        ordering::tests::ordering_is_deterministic
}

if ($OpenCascade) {
    Invoke-Gate 'opencascade clippy' { cargo clippy -p camcraft-import --features opencascade --all-targets -- -D warnings }
    Invoke-Gate 'opencascade cross-check' { cargo test -p camcraft-import --features opencascade --test opencascade }
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
