#!/usr/bin/env bash
# Validates .autonomous-team/config.json schema integrity.
# Exit 0 = valid; exit 2 = missing key or v2 schema drift detected.

set -euo pipefail

CONFIG="${VERIFY_CONFIG_PATH:-$(dirname "$0")/../config.json}"

if [[ ! -f "$CONFIG" ]]; then
  echo "Config integrity check failed: config.json not found at $CONFIG." >&2
  echo "Restore: cp .autonomous-team.archived-2026-04-24/config.json .autonomous-team/config.json" >&2
  echo "    or: re-run autonomous-team-yunxiao:init" >&2
  exit 2
fi

python3 - "$CONFIG" <<'PY'
import json, sys

config_path = sys.argv[1]
with open(config_path) as f:
    cfg = json.load(f)

def fail(key):
    print(f"Config integrity check failed: missing key '{key}'.", file=sys.stderr)
    print("Restore: cp .autonomous-team.archived-2026-04-24/config.json .autonomous-team/config.json", file=sys.stderr)
    print("    or: re-run autonomous-team-yunxiao:init", file=sys.stderr)
    sys.exit(2)

# v2 drift detection: has boss_github_username but no yunxiao block
if "boss_github_username" in cfg and "yunxiao" not in cfg:
    print("Config integrity check failed: detected v2 schema (boss_github_username present, yunxiao block absent).", file=sys.stderr)
    print("Restore: cp .autonomous-team.archived-2026-04-24/config.json .autonomous-team/config.json", file=sys.stderr)
    print("    or: re-run autonomous-team-yunxiao:init", file=sys.stderr)
    sys.exit(2)

# Required top-level and nested keys
required_paths = [
    ("yunxiao", "org_id"),
    ("yunxiao", "project_id"),
    ("yunxiao", "subject_prefix"),
    ("code_backend",),
    ("review_labels",),
]

for path in required_paths:
    node = cfg
    key_str = ".".join(path)
    for key in path:
        if not isinstance(node, dict) or key not in node:
            fail(key_str)
        node = node[key]

# code_backend-specific block check
backend = cfg.get("code_backend", "")
if backend == "gitee":
    if "gitee" not in cfg:
        fail("gitee")
elif backend == "codeup":
    if "codeup" not in cfg:
        fail("codeup")
else:
    # unknown backend — flag as missing
    if "gitee" not in cfg and "codeup" not in cfg:
        fail("gitee' or 'codeup")

sys.exit(0)
PY
