#!/usr/bin/env bash
# Serve the inspector locally and open it.
#
# A plain file:// page cannot load ES modules, so this serves the viewer over
# localhost. Nothing leaves the machine: the part geometry is loaded by dropping the
# JSON onto the page (or passing ?data=_name.json for a file inside viewer/), and is never uploaded anywhere.
set -euo pipefail
cd "$(dirname "$0")/../viewer"
PORT="${1:-8787}"
echo "inspector on http://localhost:$PORT  (ctrl-c to stop)"
( sleep 1; (command -v explorer.exe >/dev/null && explorer.exe "http://localhost:$PORT") \
  || (command -v xdg-open >/dev/null && xdg-open "http://localhost:$PORT") \
  || (command -v open >/dev/null && open "http://localhost:$PORT") || true ) &
# macOS ships python3 and no python. On Windows, python3 can be the Microsoft Store
# stub, so the plain name is tried first.
PY="$(command -v python || command -v python3)" || { echo "needs python3 on PATH" >&2; exit 1; }
"$PY" -m http.server "$PORT" --bind 127.0.0.1
