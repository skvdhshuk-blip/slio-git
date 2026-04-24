#!/usr/bin/env bash
# PR backend dispatcher — reads config.json code_backend, delegates to gitee.sh or codeup.sh.
# All agents / team-lead talk to pr.sh; concrete backend is transparent.
#
# Usage: pr.sh <command> [args...]
# Commands (common API across backends):
#   pr-list
#   pr-view <n>
#   pr-create <head_branch> <title> <body_file>
#   pr-labels <n>
#   pr-label-add <n> <label>
#   pr-label-rm <n> <label>
#   pr-comments <n>
#   pr-comment <n> <text>
#   pr-merge <n>
#   remote-url
set -euo pipefail

CONFIG_DIR="$(dirname "$0")"
"$CONFIG_DIR/verify-config.sh" || exit 2

CONFIG="$CONFIG_DIR/../config.json"
[[ -f "$CONFIG" ]] || { echo "pr.sh: config.json missing at $CONFIG" >&2; exit 2; }

backend=$(python3 -c "import json;print(json.load(open('$CONFIG'))['code_backend'])")
script_dir="$CONFIG_DIR"

case "$backend" in
  gitee)  exec bash "$script_dir/gitee.sh" "$@" ;;
  codeup) exec bash "$script_dir/codeup.sh" "$@" ;;
  *)      echo "pr.sh: unknown code_backend '$backend' (expected: gitee|codeup)" >&2; exit 2 ;;
esac
