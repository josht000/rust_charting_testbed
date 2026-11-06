# Dev server for the wasm build. http://localhost:5172  (add ?bench=1 for bench)
Set-Location $PSScriptRoot
trunk serve --port 5172 @args
