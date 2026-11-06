# Serve the wasm build at http://localhost:5173 (see serve.sh).
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
if (-not (Test-Path pkg/slint_testbed.js)) {
    Write-Host "pkg/ missing - building wasm first..."
    ./build-web.ps1
}
cargo run --release --bin serve
