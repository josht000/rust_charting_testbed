#!/usr/bin/env bash
# Serve the wasm build at http://localhost:5173.
# Uses the bundled std-only Rust server (also handles POST /save for the bench).
# Any static server works too, e.g.:  npx serve -l 5173 .   or   python -m http.server 5173
set -euo pipefail
cd "$(dirname "$0")"
if [ ! -f pkg/slint_testbed.js ]; then
    echo "pkg/ missing — building wasm first..."
    ./build-web.sh
fi
exec cargo run --release --bin serve
