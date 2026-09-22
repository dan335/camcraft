#!/usr/bin/env bash
# Every gate that guards this repository, run locally.
#
# There is no CI while the repository is private, so this script is the only thing
# standing between a mistake and main. It therefore checks each gate on its own exit
# status and reports a summary at the end, rather than chaining commands in a way
# that can report success without having checked anything.
#
#   bash scripts/check.sh           fmt, clippy, tests, docs, determinism
#   bash scripts/check.sh --full    the above plus benchmarks (slow, release build)

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

FULL=0
[ "${1:-}" = "--full" ] && FULL=1

PASS=()
FAIL=()

run() {
  local name="$1"
  shift
  printf '\n\033[1m== %s ==\033[0m\n' "$name"
  if "$@"; then
    PASS+=("$name")
  else
    FAIL+=("$name")
  fi
}

run "fmt"    cargo fmt --all --check
run "clippy" cargo clippy --workspace --all-targets -- -D warnings
run "test"   cargo test --workspace --all-targets
run "doc"    cargo doc --workspace --no-deps

# Identical inputs must produce identical results on any core count. Without this,
# golden-file comparisons are flaky and real regressions become invisible.
printf '\n\033[1m== determinism ==\033[0m\n'
if RAYON_NUM_THREADS=1 cargo test --workspace --quiet >/dev/null 2>&1 &&
   RAYON_NUM_THREADS=8 cargo test --workspace --quiet >/dev/null 2>&1; then
  PASS+=("determinism")
  echo "tests agree at 1 and 8 threads"
else
  FAIL+=("determinism")
  echo "tests differ across thread counts"
fi

if [ "$FULL" = "1" ]; then
  run "bench" cargo bench --workspace --no-fail-fast
fi

printf '\n\033[1m== summary ==\033[0m\n'
for p in "${PASS[@]:-}"; do [ -n "$p" ] && printf '  \033[32mpass\033[0m  %s\n' "$p"; done
for f in "${FAIL[@]:-}"; do [ -n "$f" ] && printf '  \033[31mFAIL\033[0m  %s\n' "$f"; done

if [ "${#FAIL[@]}" -gt 0 ]; then
  printf '\n%d gate(s) failed.\n' "${#FAIL[@]}"
  exit 1
fi
printf '\nAll gates passed.\n'
