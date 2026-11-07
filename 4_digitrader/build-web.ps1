$ErrorActionPreference = "Stop"

if (-not (Get-Command wasm-pack -ErrorAction SilentlyContinue)) {
    throw "wasm-pack is required. Install it with: cargo install wasm-pack"
}

wasm-pack build --release --target web --no-default-features --features slint/backend-winit,slint/renderer-femtovg
if ($LASTEXITCODE -ne 0) {
    throw "wasm-pack build failed"
}
