#!/usr/bin/env bash
#
# Build release artifacts locally and publish them to a Gitee Release.
#
# Usage:
#   scripts/release-gitee.sh "0.0.30" "slio-git 0.0.30"
#
# Required env:
#   GITEE_TOKEN                 Personal access token with 'projects' scope.
#                               Falls back to ~/.config/slio-git/gitee-token.
#
# Optional env:
#   GITEE_OWNER=sk-wang-sh
#   GITEE_REPO=slio-git
#   GITEE_REMOTE=gitee
#   SKIP_MACOS_ARM64=1          Skip building the named target.
#   SKIP_MACOS_X86_64=1
#   SKIP_LINUX_X86_64=1
#   SKIP_LINUX_ARM64=1
#   SKIP_WINDOWS_X86_64=1
#   SKIP_TESTS=1                Skip 'cargo test --workspace --locked'.
#   SKIP_PUSH=1                 Skip pushing the tag to the Gitee remote.
#   DRY_RUN=1                   Build + checksum only; don't create the Release.
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <version> <release-title>" >&2
  echo "Example: $0 0.0.30 'slio-git 0.0.30'" >&2
  exit 2
fi

VERSION_INPUT="$1"
RELEASE_TITLE="$2"

if [[ "$VERSION_INPUT" == v* ]]; then
  echo "Version must not start with 'v' (got $VERSION_INPUT)" >&2
  exit 2
fi

CARGO_VERSION="$(
python3 - <<'PY'
from pathlib import Path
import re
import sys

text = Path("Cargo.toml").read_text()
match = re.search(
    r"\[workspace\.package\](?:.*?\n)*?version\s*=\s*\"([^\"]+)\"",
    text,
    re.S,
)
if not match:
    sys.exit("Failed to read workspace version from Cargo.toml")
print(match.group(1))
PY
)"

if [[ "$VERSION_INPUT" != "$CARGO_VERSION" ]]; then
  echo "Version $VERSION_INPUT does not match Cargo.toml workspace version $CARGO_VERSION" >&2
  exit 2
fi

VERSION="$VERSION_INPUT"
TAG="v$VERSION"

GITEE_OWNER="${GITEE_OWNER:-sk-wang-sh}"
GITEE_REPO="${GITEE_REPO:-slio-git}"
GITEE_REMOTE="${GITEE_REMOTE:-gitee}"
GITEE_API="https://gitee.com/api/v5"

# ---- Token ------------------------------------------------------------------
TOKEN_FILE="${HOME}/.config/slio-git/gitee-token"
if [[ -z "${GITEE_TOKEN:-}" && -r "$TOKEN_FILE" ]]; then
  GITEE_TOKEN="$(tr -d '[:space:]' < "$TOKEN_FILE")"
fi
if [[ -z "${GITEE_TOKEN:-}" && "${DRY_RUN:-0}" != "1" ]]; then
  echo "GITEE_TOKEN not set and $TOKEN_FILE not readable." >&2
  echo "Create a token at https://gitee.com/profile/personal_access_tokens (scope: projects)." >&2
  exit 2
fi

# ---- Git safety -------------------------------------------------------------
# Only tracked-file changes block a release; untracked files are ignored.
if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
  echo "Tracked files have uncommitted changes; commit or stash before releasing." >&2
  git status --short --untracked-files=no >&2
  exit 2
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Tag $TAG already exists locally. Delete it first if you want to re-cut." >&2
  exit 2
fi

if ! git remote get-url "$GITEE_REMOTE" >/dev/null 2>&1; then
  echo "Git remote '$GITEE_REMOTE' is not configured." >&2
  exit 2
fi

# ---- Platform flags ---------------------------------------------------------
ENABLE_MACOS_ARM64=${SKIP_MACOS_ARM64:+0}; ENABLE_MACOS_ARM64=${ENABLE_MACOS_ARM64:-1}
ENABLE_MACOS_X86_64=${SKIP_MACOS_X86_64:+0}; ENABLE_MACOS_X86_64=${ENABLE_MACOS_X86_64:-1}
ENABLE_LINUX_X86_64=${SKIP_LINUX_X86_64:+0}; ENABLE_LINUX_X86_64=${ENABLE_LINUX_X86_64:-1}
ENABLE_LINUX_ARM64=${SKIP_LINUX_ARM64:+0}; ENABLE_LINUX_ARM64=${ENABLE_LINUX_ARM64:-1}
ENABLE_WINDOWS_X86_64=${SKIP_WINDOWS_X86_64:+0}; ENABLE_WINDOWS_X86_64=${ENABLE_WINDOWS_X86_64:-1}

need_zigbuild=0
if [[ "$ENABLE_LINUX_X86_64" == "1" || "$ENABLE_LINUX_ARM64" == "1" ]]; then
  need_zigbuild=1
fi
if [[ "$need_zigbuild" == "1" ]]; then
  if ! command -v zig >/dev/null 2>&1; then
    echo "zig is required for Linux targets. Install with: brew install zig" >&2
    exit 2
  fi
  if ! command -v cargo-zigbuild >/dev/null 2>&1; then
    echo "cargo-zigbuild is required for Linux targets. Install with: cargo install cargo-zigbuild" >&2
    exit 2
  fi
fi

if [[ "$ENABLE_WINDOWS_X86_64" == "1" ]] && ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  echo "x86_64-w64-mingw32-gcc missing. Install with: brew install mingw-w64" >&2
  exit 2
fi

# ---- Tests ------------------------------------------------------------------
if [[ "${SKIP_TESTS:-0}" != "1" ]]; then
  echo "==> cargo test --workspace --locked"
  cargo test --workspace --locked
fi

# ---- Build ------------------------------------------------------------------
DIST_DIR="$ROOT_DIR/dist"
PUBLISH_DIR="$DIST_DIR/publish"
rm -rf "$PUBLISH_DIR"
mkdir -p "$PUBLISH_DIR"

ARTIFACTS=()

build_macos() {
  local target="$1" arch="$2" file="slio-git-macos-$2.dmg"
  echo "==> Building $file"
  MACOS_TARGET="$target" MACOS_ARCH="$arch" bash scripts/package-macos-dmg.sh
  cp "$DIST_DIR/$file" "$PUBLISH_DIR/"
  ARTIFACTS+=("$file")
}

build_linux() {
  local target="$1" arch="$2" file="slio-git-linux-$2.tar.gz"
  echo "==> Building $file"
  LINUX_TARGET="$target" LINUX_ARCH="$arch" \
    CARGO_BUILD_CMD="cargo zigbuild" \
    bash scripts/package-linux-tarball.sh
  cp "$DIST_DIR/$file" "$PUBLISH_DIR/"
  ARTIFACTS+=("$file")
}

build_windows() {
  local target="$1" arch="$2" file="slio-git-windows-$2.zip"
  echo "==> Building $file"
  WINDOWS_TARGET="$target" WINDOWS_ARCH="$arch" bash scripts/package-windows-zip.sh
  cp "$DIST_DIR/$file" "$PUBLISH_DIR/"
  ARTIFACTS+=("$file")
}

[[ "$ENABLE_MACOS_ARM64"   == "1" ]] && build_macos   aarch64-apple-darwin       aarch64
[[ "$ENABLE_MACOS_X86_64"  == "1" ]] && build_macos   x86_64-apple-darwin        x86_64
[[ "$ENABLE_LINUX_X86_64"  == "1" ]] && build_linux   x86_64-unknown-linux-gnu   x86_64
[[ "$ENABLE_LINUX_ARM64"   == "1" ]] && build_linux   aarch64-unknown-linux-gnu  aarch64
[[ "$ENABLE_WINDOWS_X86_64" == "1" ]] && build_windows x86_64-pc-windows-gnu     x86_64

if [[ ${#ARTIFACTS[@]} -eq 0 ]]; then
  echo "All platforms were skipped; nothing to publish." >&2
  exit 2
fi

# ---- Checksums + release notes ---------------------------------------------
(
  cd "$PUBLISH_DIR"
  shasum -a 256 slio-git-* > SHA256SUMS.txt
)

PREVIOUS_TAG="$(git tag --sort=-v:refname | grep -Fvx "$TAG" | head -n 1 || true)"
BODY_FILE="$PUBLISH_DIR/RELEASE_BODY.md"
{
  printf '## %s\n\n' "$RELEASE_TITLE"
  printf '## Changelog\n\n'
  if [[ -n "$PREVIOUS_TAG" ]]; then
    printf -- '- Compare: https://gitee.com/%s/%s/compare/%s...%s\n\n' \
      "$GITEE_OWNER" "$GITEE_REPO" "$PREVIOUS_TAG" "$TAG"
  else
    printf -- '- First published release in this repository.\n\n'
  fi
  printf '## Checksums\n\n```text\n'
  cat "$PUBLISH_DIR/SHA256SUMS.txt"
  printf '```\n'
} > "$BODY_FILE"

echo "==> Built artifacts:"
printf '  - %s\n' "${ARTIFACTS[@]}" SHA256SUMS.txt

if [[ "${DRY_RUN:-0}" == "1" ]]; then
  echo "DRY_RUN=1 — stopping before tag/push/publish."
  echo "Artifacts in: $PUBLISH_DIR"
  exit 0
fi

# ---- Tag + push -------------------------------------------------------------
echo "==> git tag $TAG"
git tag -a "$TAG" -m "$RELEASE_TITLE"

if [[ "${SKIP_PUSH:-0}" != "1" ]]; then
  echo "==> git push $GITEE_REMOTE HEAD $TAG"
  CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
  git push "$GITEE_REMOTE" "$CURRENT_BRANCH"
  git push "$GITEE_REMOTE" "$TAG"
fi

# ---- Gitee Release API ------------------------------------------------------
echo "==> Creating Gitee release $TAG"
TARGET_COMMIT="$(git rev-parse HEAD)"

CREATE_PAYLOAD="$(python3 - "$TAG" "$RELEASE_TITLE" "$BODY_FILE" "$TARGET_COMMIT" <<'PY'
import json
import sys
from pathlib import Path

tag, title, body_path, commit = sys.argv[1:5]
body = Path(body_path).read_text(encoding="utf-8")
print(json.dumps({
    "tag_name": tag,
    "name": title,
    "body": body,
    "prerelease": False,
    "target_commitish": commit,
}))
PY
)"

CREATE_RESPONSE="$(mktemp)"
trap 'rm -f "$CREATE_RESPONSE"' EXIT

HTTP_CODE="$(
  curl -sS -o "$CREATE_RESPONSE" -w '%{http_code}' \
    -X POST "$GITEE_API/repos/$GITEE_OWNER/$GITEE_REPO/releases" \
    -H "Content-Type: application/json;charset=UTF-8" \
    -H "Authorization: token $GITEE_TOKEN" \
    -d "$CREATE_PAYLOAD"
)"

if [[ "$HTTP_CODE" != "201" && "$HTTP_CODE" != "200" ]]; then
  echo "Gitee release creation failed (HTTP $HTTP_CODE):" >&2
  cat "$CREATE_RESPONSE" >&2
  echo >&2
  exit 1
fi

RELEASE_ID="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["id"])' "$CREATE_RESPONSE")"
echo "Gitee release id: $RELEASE_ID"

upload_one() {
  local path="$1" name
  name="$(basename "$path")"
  echo "  uploading $name"
  local resp http
  resp="$(mktemp)"
  http="$(
    curl -sS -o "$resp" -w '%{http_code}' \
      -X POST "$GITEE_API/repos/$GITEE_OWNER/$GITEE_REPO/releases/$RELEASE_ID/attach_files" \
      -H "Authorization: token $GITEE_TOKEN" \
      -F "file=@$path"
  )"
  if [[ "$http" != "201" && "$http" != "200" ]]; then
    echo "    upload failed (HTTP $http):" >&2
    cat "$resp" >&2
    echo >&2
    rm -f "$resp"
    return 1
  fi
  rm -f "$resp"
}

for name in "${ARTIFACTS[@]}" SHA256SUMS.txt; do
  upload_one "$PUBLISH_DIR/$name"
done

echo "Done. Release: https://gitee.com/$GITEE_OWNER/$GITEE_REPO/releases/tag/$TAG"
