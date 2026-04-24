#!/usr/bin/env bash
# Smoke test: verify yx.sh contains UNTRUSTED CONTENT fence output in the
# get/comments/create-topic/create-bug branches; list TSV must NOT have fence.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
YX="$SCRIPT_DIR/../../scripts/yx.sh"
PASS=0
FAIL=0

check_grep() {
  local label="$1"
  local pattern="$2"
  if grep -q "$pattern" "$YX"; then
    echo "PASS: $label"
    PASS=$((PASS + 1))
  else
    echo "FAIL: $label — pattern '$pattern' not found in yx.sh"
    FAIL=$((FAIL + 1))
  fi
}

# R1: get/comments/create-topic+create-bug must emit fence sentinels
check_grep "get emits BEGIN fence"      "BEGIN UNTRUSTED CONTENT"
check_grep "get emits END fence"        "END UNTRUSTED CONTENT"

# Verify get/comments/create-topic+bug all have echo fence lines (>=3 distinct call sites)
count_begin=$(grep -c "BEGIN UNTRUSTED CONTENT" "$YX" || true)
count_end=$(grep -c "END UNTRUSTED CONTENT" "$YX" || true)

if [[ "$count_begin" -ge 3 ]]; then
  echo "PASS: BEGIN fence appears $count_begin times (>=3: get/comments/create covered)"
  PASS=$((PASS + 1))
else
  echo "FAIL: BEGIN fence only appears $count_begin times (expected >=3)"
  FAIL=$((FAIL + 1))
fi

if [[ "$count_end" -ge 3 ]]; then
  echo "PASS: END fence appears $count_end times (>=3: get/comments/create covered)"
  PASS=$((PASS + 1))
else
  echo "FAIL: END fence only appears $count_end times (expected >=3)"
  FAIL=$((FAIL + 1))
fi

# AC4: list TSV must NOT embed fence in the subject field (would break NF=4)
if ! grep -A30 '  list)' "$YX" | grep -q "UNTRUSTED CONTENT"; then
  echo "PASS: list branch does not embed UNTRUSTED CONTENT fence (TSV NF=4 safe)"
  PASS=$((PASS + 1))
else
  echo "FAIL: list branch contains UNTRUSTED CONTENT fence — breaks TSV NF=4"
  FAIL=$((FAIL + 1))
fi

# Verify status-line does NOT have fence (machine-readable command)
if ! grep -A5 'status-line)' "$YX" | grep -q "UNTRUSTED CONTENT"; then
  echo "PASS: status-line does not emit fence"
  PASS=$((PASS + 1))
else
  echo "FAIL: status-line should not emit fence"
  FAIL=$((FAIL + 1))
fi

echo ""
echo "fence smoke tests: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
