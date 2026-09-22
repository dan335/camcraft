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

# Identical inputs must produce identical results on any core count.
#
# This gate used to run the whole suite twice, at one thread and at eight. That was
# expensive -- it tripled an already slow run -- and, worse, it did not test what it
# claimed. Running a deterministic test at two thread counts just passes twice; it
# never compares one run against the other, so it could not have detected
# non-determinism at all.
#
# The real evidence lives in tests that compare output against itself:
# fill_is_bit_identical_regardless_of_threads builds the same CL map at one and
# eight threads and diffs it bit for bit. That is direct, and it takes seconds.
run "determinism" cargo test --workspace --quiet -- --exact   clmap::tests::fill_is_bit_identical_regardless_of_threads   dropcutter::tests::is_deterministic   ordering::tests::ordering_is_deterministic

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
