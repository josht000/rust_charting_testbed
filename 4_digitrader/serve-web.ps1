$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if (-not (Test-Path "pkg/digitrader.js")) {
    throw "Web bundle not found. Run .\\build-web.ps1 first."
}

if (Get-Command py -ErrorAction SilentlyContinue) {
    py -3 -m http.server 5174
} elseif (Get-Command python3 -ErrorAction SilentlyContinue) {
    python3 -m http.server 5174
} else {
    throw "Python 3 is required to serve the web bundle."
}

if ($LASTEXITCODE -ne 0) {
    throw "The web server exited with an error."
}
