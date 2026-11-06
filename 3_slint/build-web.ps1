# Build the wasm bundle into ./pkg and write sizes.json (see build-web.sh).
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

wasm-pack build --release --target web --out-dir pkg `
    --no-default-features --features slint/backend-winit,slint/renderer-femtovg

$wasm = (Get-Item pkg/slint_testbed_bg.wasm).Length
$js = (Get-Item pkg/slint_testbed.js).Length
$idx = (Get-Item index.html).Length
$snip = 0
if (Test-Path pkg/snippets) {
    $snip = (Get-ChildItem pkg/snippets -Recurse -File | Measure-Object Length -Sum).Sum
}
$tot = $wasm + $js + $idx + $snip
$mb = { param($b) [math]::Round($b / 1MB, 4) }

@"
{
  "wasm_bytes": $wasm,
  "js_bytes": $js,
  "total_web_dist_bytes": $tot,
  "wasm_mb": $(& $mb $wasm),
  "js_bundle_mb": $(& $mb $js),
  "total_web_dist_mb": $(& $mb $tot)
}
"@ | Set-Content sizes.json
Write-Host "wrote sizes.json"
