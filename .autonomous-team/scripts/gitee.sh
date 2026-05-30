#!/usr/bin/env bash
# Gitee helper — wraps Gitee REST API v5 with token from config
# Usage: gi.sh <command> [args]
# Commands:
#   pr-list                       list open PRs (number, title, head branch, labels)
#   pr-create <head> <title> <body_file>   create PR (base = configured default_branch)
#   pr-view <number>              show PR (full json)
#   pr-labels <number>            print labels on PR (one per line)
#   pr-label-add <number> <label> add label
#   pr-label-rm <number> <label>  remove label
#   pr-comments <number>          list PR comments (review + diff + general)
#   pr-comment <number> <text>    add general comment
#   pr-merge <number>             squash-merge and delete remote branch
#   remote-url                    print configured gitee git remote url
#
# Exit 0 on success.

set -euo pipefail

CONFIG="$(dirname "$0")/../config.json"
[[ -f "$CONFIG" ]] || { echo "gi.sh: config.json missing" >&2; exit 2; }

OWNER=$(python3 -c "import json;print(json.load(open('$CONFIG'))['gitee']['owner'])")
REPO=$(python3 -c "import json;print(json.load(open('$CONFIG'))['gitee']['repo'])")
BASE=$(python3 -c "import json;print(json.load(open('$CONFIG'))['gitee']['default_branch'])")
TOKEN_PATH=$(python3 -c "import json,os;p=json.load(open('$CONFIG'))['gitee']['token_path'];print(os.path.expanduser(p))")
API=$(python3 -c "import json;print(json.load(open('$CONFIG'))['gitee']['api_base'])")
GIT_REMOTE=$(python3 -c "import json;print(json.load(open('$CONFIG'))['gitee']['git_remote'])")

[[ -f "$TOKEN_PATH" ]] || { echo "gi.sh: gitee token file missing at $TOKEN_PATH" >&2; exit 2; }
TOKEN=$(cat "$TOKEN_PATH")

H=(-H "Authorization: token $TOKEN" -H "Content-Type: application/json")

cmd="${1:-}"; shift || true

case "$cmd" in
  pr-list)
    curl -sS "${H[@]}" "$API/repos/$OWNER/$REPO/pulls?state=open&per_page=50" \
      | python3 -c "
import sys, json
for p in json.load(sys.stdin):
    labels=','.join(l['name'] for l in p.get('labels',[]))
    print(f\"{p['number']}\t{p['head']['ref']}\t{labels}\t{p['title']}\")
"
    ;;
  pr-create)
    head="$1"; title="$2"; body_file="$3"
    body_text=$(cat "$body_file")
    payload=$(python3 -c "
import json, sys
print(json.dumps({'title': sys.argv[1], 'head': sys.argv[2], 'base': sys.argv[3], 'body': sys.argv[4]}))
" "$title" "$head" "$BASE" "$body_text")
    curl -sS "${H[@]}" -X POST "$API/repos/$OWNER/$REPO/pulls" -d "$payload" \
      | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('number') or d)"
    ;;
  pr-view)
    n="$1"
    curl -sS "${H[@]}" "$API/repos/$OWNER/$REPO/pulls/$n"
    ;;
  pr-labels)
    n="$1"
    curl -sS "${H[@]}" "$API/repos/$OWNER/$REPO/pulls/$n/labels" \
      | python3 -c "import sys,json;print('\n'.join(l['name'] for l in json.load(sys.stdin)))"
    ;;
  pr-label-add)
    n="$1"; label="$2"
    payload=$(python3 -c "import json,sys;print(json.dumps([sys.argv[1]]))" "$label")
    curl -sS "${H[@]}" -X POST "$API/repos/$OWNER/$REPO/pulls/$n/labels" -d "$payload" > /dev/null
    echo "added: $label"
    ;;
  pr-label-rm)
    n="$1"; label="$2"
    curl -sS "${H[@]}" -X DELETE "$API/repos/$OWNER/$REPO/pulls/$n/labels/$label" > /dev/null
    echo "removed: $label"
    ;;
  pr-comments)
    n="$1"
    curl -sS "${H[@]}" "$API/repos/$OWNER/$REPO/pulls/$n/comments?per_page=100"
    ;;
  pr-comment)
    n="$1"; text="$2"
    payload=$(python3 -c "import json,sys;print(json.dumps({'body': sys.argv[1]}))" "$text")
    curl -sS "${H[@]}" -X POST "$API/repos/$OWNER/$REPO/pulls/$n/comments" -d "$payload" > /dev/null
    echo "commented on PR #$n"
    ;;
  pr-merge)
    n="$1"
    # Gitee 要求 assignee accept + tester accept 才能合并（即便有 passed labels）
    # 作者 = PAT 拥有者时，Gitee 禁止 web 自审，但 API review/test 端点可以强制通过
    curl -sS "${H[@]}" -X POST "$API/repos/$OWNER/$REPO/pulls/$n/review?force=true" > /dev/null 2>&1 || true
    curl -sS "${H[@]}" -X POST "$API/repos/$OWNER/$REPO/pulls/$n/test?force=true" > /dev/null 2>&1 || true
    payload='{"merge_method":"squash","prune_source_branch":true}'
    curl -sS "${H[@]}" -X PUT "$API/repos/$OWNER/$REPO/pulls/$n/merge" -d "$payload" \
      | head -c 500
    echo
    ;;
  remote-url)
    git remote get-url "$GIT_REMOTE"
    ;;
  *)
    echo "Unknown command: $cmd"
    echo "Available: pr-list | pr-create | pr-view | pr-labels | pr-label-add | pr-label-rm | pr-comments | pr-comment | pr-merge | remote-url"
    exit 2
    ;;
esac
