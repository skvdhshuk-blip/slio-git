#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

APP_NAME="slio-git"
DIST_DIR="$ROOT_DIR/dist"
STAGING_DIR="$DIST_DIR/linux-root"
PACKAGE_DIR="$STAGING_DIR/${APP_NAME}-linux-x86_64"
TARBALL_PATH="$DIST_DIR/${APP_NAME}-linux-x86_64.tar.gz"

VERSION="$(
python3 - <<'PY'
from pathlib import Path
import re

text = Path("Cargo.toml").read_text()
match = re.search(
    r"\[workspace\.package\](?:.*?\n)*?version\s*=\s*\"([^\"]+)\"",
    text,
    re.S,
)
if not match:
    raise SystemExit("Failed to read workspace version from Cargo.toml")
print(match.group(1))
PY
)"

echo "Building Linux release binary..."
cargo build --locked --release -p src-ui

echo "Preparing tarball contents..."
rm -rf "$STAGING_DIR"
mkdir -p "$PACKAGE_DIR"

cp "$ROOT_DIR/target/release/src-ui" "$PACKAGE_DIR/$APP_NAME"
chmod 755 "$PACKAGE_DIR/$APP_NAME"

cat > "$PACKAGE_DIR/README.txt" <<EOF
slio-git ${VERSION}
=================

1. Extract this archive.
2. Run ./${APP_NAME}.

Source:
https://github.com/sk-wang/slio-git
EOF

echo "Creating tar.gz archive..."
rm -f "$TARBALL_PATH"
tar -C "$STAGING_DIR" -czf "$TARBALL_PATH" "$(basename "$PACKAGE_DIR")"

echo "Done:"
echo "  Tarball: $TARBALL_PATH"
