#!/usr/bin/env bash
# Build and run the GUI in release mode.
#
#   bash scripts/run.sh [args...]   any arguments are passed through to the app
set -euo pipefail
cd "$(dirname "$0")/.."
exec cargo run --release -p camcraft-gui --bin camcraft-gui -- "$@"
