#!/usr/bin/env bash
# N3 test suite runner — verify-config + fence smoke tests
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FAIL=0

run() {
  local name="$1"
  local script="$2"
  echo "=== $name ==="
  if bash "$script"; then
    echo "=== $name: PASS ==="
  else
    echo "=== $name: FAIL ==="
    FAIL=$((FAIL + 1))
  fi
  echo ""
}

run "test_verify_config" "$SCRIPT_DIR/test_verify_config.sh"
run "test_fence_smoke"   "$SCRIPT_DIR/test_fence_smoke.sh"

if [[ "$FAIL" -gt 0 ]]; then
  echo "N3 suite: $FAIL test(s) FAILED"
  exit 1
fi
echo "N3 suite: all tests passed"
