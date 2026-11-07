#!/usr/bin/env sh
set -eu
command -v wasm-pack >/dev/null 2>&1 || {
    echo "wasm-pack is required. Install it with: cargo install wasm-pack" >&2
    exit 1
}
wasm-pack build --release --target web --no-default-features --features slint/backend-winit,slint/renderer-femtovg
