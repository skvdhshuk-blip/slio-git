#!/usr/bin/env bash
# Tests for verify-config.sh: 3 fixtures → assert exit 2/2/0
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VERIFY="$SCRIPT_DIR/../../scripts/verify-config.sh"
PASS=0
FAIL=0

run_test() {
  local name="$1"
  local fixture="$2"
  local expected_exit="$3"

  actual_exit=0
  VERIFY_CONFIG_PATH="$fixture" bash "$VERIFY" 2>/dev/null || actual_exit=$?

  if [[ "$actual_exit" -eq "$expected_exit" ]]; then
    echo "PASS: $name (exit $actual_exit)"
    PASS=$((PASS + 1))
  else
    echo "FAIL: $name (expected exit $expected_exit, got $actual_exit)"
    FAIL=$((FAIL + 1))
  fi
}

run_test "v2_schema exits 2"       "$SCRIPT_DIR/fixture_v2_schema.json"       2
run_test "missing_yunxiao exits 2" "$SCRIPT_DIR/fixture_missing_yunxiao.json" 2
run_test "valid exits 0"           "$SCRIPT_DIR/fixture_valid.json"            0

echo ""
echo "verify-config tests: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
