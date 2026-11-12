#!/usr/bin/env bash
# Run a Python script inside Fusion, through the CamcraftBridge add-in, and print
# what it printed (or its traceback).
#
#   FUSION_HOST=user@mac bash scripts/fusion/run.sh scripts/fusion/ops_block/part.py
#
# With FUSION_HOST, the script goes over SSH to that machine's bridge folder,
# ~/camcraft-fusion-bridge; without it, to this machine's (%LOCALAPPDATA% on
# Windows, ~ elsewhere). See README.md.
set -euo pipefail

script="${1:?usage: run.sh SCRIPT.py [timeout seconds]}"
limit="${2:-600}"
name="$(basename "$script")"

if [ -n "${FUSION_HOST:-}" ]; then
  remote() { ssh -o BatchMode=yes "$FUSION_HOST" "$@"; }
  dir='~/camcraft-fusion-bridge'
else
  remote() { bash -c "$*"; }
  if [ -n "${LOCALAPPDATA:-}" ]; then
    dir="$(cygpath -u "$LOCALAPPDATA" 2>/dev/null || echo "$LOCALAPPDATA")/camcraft-fusion-bridge"
  else
    dir="$HOME/camcraft-fusion-bridge"
  fi
fi

# Written under another name first, so the bridge never runs half a script.
remote "rm -f $dir/outbox/$name.ok.txt $dir/outbox/$name.err.txt; \
  cat > $dir/inbox/$name.tmp && mv $dir/inbox/$name.tmp $dir/inbox/$name" < "$script"

start=$(date +%s)
while :; do
  out=$(remote "cd $dir/outbox; for s in ok err; do \
    if [ -f $name.\$s.txt ]; then echo \"== \$s\"; cat $name.\$s.txt; fi; done")
  if [ -n "$out" ]; then
    echo "$out"
    case "$out" in "== err"*) exit 1 ;; esac
    exit 0
  fi
  if [ $(($(date +%s) - start)) -gt "$limit" ]; then
    echo "no answer in ${limit}s: is CamcraftBridge running in Fusion?" >&2
    exit 2
  fi
  sleep 2
done
