#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")"
test -f pkg/digitrader.js || {
    echo "Web bundle not found. Run ./build-web.sh first." >&2
    exit 1
}
exec python3 -m http.server 5174
