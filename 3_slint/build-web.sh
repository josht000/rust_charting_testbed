#!/usr/bin/env bash
# Build the wasm bundle (winit + femtovg/WebGL backend only) into ./pkg
# and write sizes.json consumed by the web benchmark.
set -euo pipefail
cd "$(dirname "$0")"

wasm-pack build --release --target web --out-dir pkg \
    --no-default-features --features slint/backend-winit,slint/renderer-femtovg

mb() { awk -v b="$1" 'BEGIN{printf "%.4f", b/1048576}'; }

WASM_B=$(stat -c%s pkg/slint_testbed_bg.wasm)
JS_B=$(stat -c%s pkg/slint_testbed.js)
IDX_B=$(stat -c%s index.html)
SNIP_B=0
if [ -d pkg/snippets ]; then SNIP_B=$(find pkg/snippets -type f -exec cat {} + | wc -c); fi
TOT=$((WASM_B + JS_B + IDX_B + SNIP_B))

cat > sizes.json <<EOF
{
  "wasm_bytes": $WASM_B,
  "js_bytes": $JS_B,
  "total_web_dist_bytes": $TOT,
  "wasm_mb": $(mb "$WASM_B"),
  "js_bundle_mb": $(mb "$JS_B"),
  "total_web_dist_mb": $(mb "$TOT")
}
EOF
echo "wrote sizes.json: wasm=$(mb "$WASM_B") MB js=$(mb "$JS_B") MB total=$(mb "$TOT") MB"
