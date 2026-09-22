#!/usr/bin/env bash
# Serve the inspector locally and open it.
#
# A plain file:// page cannot load ES modules, so this serves the viewer over
# localhost. Nothing leaves the machine: the part geometry is loaded by dropping the
# JSON onto the page, and is never uploaded anywhere.
set -euo pipefail
cd "$(dirname "$0")/../viewer"
PORT="${1:-8787}"
echo "inspector on http://localhost:$PORT  (ctrl-c to stop)"
( sleep 1; (command -v explorer.exe >/dev/null && explorer.exe "http://localhost:$PORT") \
  || (command -v xdg-open >/dev/null && xdg-open "http://localhost:$PORT") \
  || (command -v open >/dev/null && open "http://localhost:$PORT") || true ) &
python -m http.server "$PORT" --bind 127.0.0.1
