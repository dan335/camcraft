#!/usr/bin/env bash
# Every gate that guards this repository, run locally.
#
# There is no CI while the repository is private, so this script is the only thing
# standing between a mistake and main. It therefore checks each gate on its own exit
# status and reports a summary at the end, rather than chaining commands in a way
# that can report success without having checked anything.
#
#   bash scripts/check.sh               the crates your changes can affect
#   bash scripts/check.sh --since main  the same, for everything since `main`
#   bash scripts/check.sh --all         every crate: before pushing
#   bash scripts/check.sh --full        every crate, plus benchmarks (slow)
#   bash scripts/check.sh --dry-run     say what would be checked, and stop
#
# By default it checks what the uncommitted changes can affect: the crates they
# touch, and every crate that depends on one of those, found from `cargo metadata`.
# A change only in camcraft-gui checks camcraft-gui; a change in camcraft-core
# checks core and everything built on it, but not geom or sim underneath it.
#
# Anything outside crates/ that is not documentation (manifests, the lockfile,
# examples, fixtures, the toolchain, this script) is read by tests or changes how
# everything builds, so it counts as changing every crate. When in doubt, --all.
#
# scripts/check.ps1 is the Windows version and always checks everything.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

SCOPE=changed
SINCE=HEAD
BENCH=0
DRY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --all) SCOPE=all ;;
    --full) SCOPE=all; BENCH=1 ;;
    --since) SINCE="${2:?--since needs a git ref}"; shift ;;
    --dry-run) DRY=1 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

# The files in question: committed since $SINCE, changed in the working tree, and
# new files git does not track yet.
changed_files() {
  # CHECK_FILES, one path per line, stands in for git: for testing this script.
  if [ -n "${CHECK_FILES:-}" ]; then
    printf '%s\n' "$CHECK_FILES"
    return
  fi
  {
    if [ "$SINCE" != HEAD ]; then git diff --name-only "$SINCE"...HEAD; fi
    git diff --name-only HEAD
    git ls-files --others --exclude-standard
  } | sort -u
}

# The workspace crates those files can affect, one per line, or ALL.
affected() {
  changed_files | python3 -c '
import json, subprocess, sys, os
files = [l.strip() for l in sys.stdin if l.strip()]
meta = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"],
    capture_output=True, text=True, check=True).stdout)
root = meta["workspace_root"]
members = {}
for p in meta["packages"]:
    members[p["name"]] = {
        "dir": os.path.relpath(os.path.dirname(p["manifest_path"]), root),
        "deps": {d["name"] for d in p["dependencies"]},
    }
def docs_only(f):
    return f.endswith(".md") or f.startswith("docs/") or f.startswith("LICENSE")
hit = set()
for f in files:
    owner = next((n for n, m in members.items() if f.startswith(m["dir"] + "/")), None)
    if owner:
        hit.add(owner)
    elif not docs_only(f):
        print("ALL")
        sys.exit()
# Everything that depends, however indirectly, on a crate that changed.
grew = True
while grew:
    grew = False
    for n, m in members.items():
        if n not in hit and m["deps"] & hit:
            hit.add(n)
            grew = True
for n in sorted(hit):
    print(n)
'
}

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

if [ "$SCOPE" = all ]; then
  CRATES=ALL
else
  CRATES="$(affected)" || { echo "could not work out what changed; use --all" >&2; exit 2; }
fi

if [ "$CRATES" = ALL ]; then
  SELECT=(--workspace)
  printf '\033[1mchecking every crate\033[0m\n'
elif [ -z "$CRATES" ]; then
  SELECT=()
  printf '\033[1mno crate is affected by the changes\033[0m (docs only?); formatting only\n'
else
  SELECT=()
  for c in $CRATES; do SELECT+=(-p "$c"); done
  printf '\033[1mchecking:\033[0m %s\n' "$(echo $CRATES)"
fi

if [ "$DRY" = 1 ]; then
  exit 0
fi

# Formatting is checked everywhere: it takes a second.
run "fmt" cargo fmt --all --check

if [ ${#SELECT[@]} -gt 0 ]; then
  run "clippy" cargo clippy "${SELECT[@]}" --all-targets -- -D warnings
  run "test"   cargo test "${SELECT[@]}" --all-targets
  run "doc"    cargo doc "${SELECT[@]}" --no-deps

  # Identical inputs must produce identical results on any core count.
  #
  # This gate used to run the whole suite twice, at one thread and at eight. That
  # was expensive and did not test what it claimed: running a deterministic test at
  # two thread counts just passes twice. The evidence lives in tests that compare
  # output against itself: bounded_runs_are_deterministic runs a roughing kept to a
  # picked face at one and eight threads and compares the moves. Those tests are in
  # camcraft-core, so this runs when core is being checked. `--exact` passes on a
  # name that matches nothing, which is how a removed module's test sat here
  # unnoticed; check the count it reports after renaming one.
  if [ "$CRATES" = ALL ] || echo "$CRATES" | grep -qx camcraft-core; then
    run "determinism" cargo test -p camcraft-core --quiet -- --exact \
      run::tests::bounded_runs_are_deterministic \
      dropcutter::tests::is_deterministic \
      ordering::tests::ordering_is_deterministic
  fi
fi

if [ "$BENCH" = "1" ]; then
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
