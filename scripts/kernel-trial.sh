#!/usr/bin/env bash
# Score STEP readers on camcraft's parts with step-conformance
# (https://github.com/dan335/step-conformance), the shared test suite that grew out
# of judging replacements for OpenCascade. Its corpus holds the committed fixtures;
# CAMCRAFT_FIXTURES, when set, is scored too but not recorded. See
# docs/decisions/0001-step-backend.md for what the last run found.
#
#   bash scripts/kernel-trial.sh

set -euo pipefail
cd "$(dirname "$0")/.."

T=target/step-conformance
if [ ! -d "$T/.git" ]; then
  git clone --quiet https://github.com/dan335/step-conformance.git "$T"
fi
git -C "$T" pull --quiet --ff-only

EXTRA=()
if [ -n "${CAMCRAFT_FIXTURES:-}" ]; then
  EXTRA+=("$(cd "$CAMCRAFT_FIXTURES" && pwd)")
fi
exec "$T/run.sh" ${EXTRA[@]+"${EXTRA[@]}"}
