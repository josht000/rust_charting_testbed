#!/usr/bin/env bash
# Dev server for the wasm build. http://localhost:5172  (add ?bench=1 for bench)
set -e
cd "$(dirname "$0")"
exec trunk serve --port 5172 "$@"
