#!/usr/bin/env bash
# Yunxiao Projex helper — wraps ~/.claude/skills/yunxiao/scripts/yunxiao_api.py
# Every work item belongs to the configured Projex project; subject prefixed [hao-code]
# Usage: yx.sh <command> [args]
# Commands:
#   list                     list open work items in our scope (subject starts with [hao-code])
#   get <id>                 fetch one work item (id + body + status marker)
#   comments <id>            list comments on a work item
#   comment <id> <text>      add a comment
#   create-topic <title> <body_file>
#                            create a Task-category work item, returns id
#   create-bug <title> <body_file>
#                            create a Bug-category work item, returns id
#   body-set <id> <body_file>
#                            replace description with body_file contents (used for STATUS marker updates)
#   status-line <id>         print first-line STATUS marker from body (machine-readable)
#   last-updated <id>        print gmtModified (epoch ms)
#
# Exit 0 on success; non-zero on API error.

set -euo pipefail

CONFIG_DIR="$(dirname "$0")"
"$CONFIG_DIR/verify-config.sh" || exit 2

CONFIG="$CONFIG_DIR/../config.json"
if [[ ! -f "$CONFIG" ]]; then echo "yx.sh: config.json missing" >&2; exit 2; fi

SPACE_ID=$(python3 -c "import json;print(json.load(open('$CONFIG'))['yunxiao']['project_id'])")
TASK_TYPE=$(python3 -c "import json;print(json.load(open('$CONFIG'))['yunxiao']['workitem_type_task'])")
BUG_TYPE=$(python3 -c "import json;print(json.load(open('$CONFIG'))['yunxiao']['workitem_type_bug'])")
PREFIX=$(python3 -c "import json;print(json.load(open('$CONFIG'))['yunxiao']['subject_prefix'])")
ASSIGNEE=$(python3 -c "import json;print(json.load(open('$CONFIG'))['boss']['yunxiao_user_id'])")
API="python3 $HOME/.claude/skills/yunxiao/scripts/yunxiao_api.py request"

cmd="${1:-}"; shift || true

case "$cmd" in
  list)
    # List all work items in our project, both Task and Bug categories, subject starts with prefix
    for cat in Task Bug; do
      body=$(CAT="$cat" SPACE_ID="$SPACE_ID" PREFIX="$PREFIX" python3 -c '
import os, json
print(json.dumps({
    "category": os.environ["CAT"],
    "spaceId": os.environ["SPACE_ID"],
    "page": 1,
    "perPage": 100,
    "orderBy": "gmtModified",
    "sort": "desc",
    "conditions": json.dumps({"conditionGroups":[[{"fieldIdentifier":"subject","operator":"CONTAINS","value":[os.environ["PREFIX"]],"className":"string","format":"input"}]]})
}))
')
      $API "/oapi/v1/projex/organizations/{orgId}/workitems:search" -X POST --body "$body"
      echo "---CHUNK---"
    done | python3 -c '
import sys, json
text = sys.stdin.read()
chunks = text.split("---CHUNK---")
seen = set()
for chunk in chunks:
    chunk = chunk.strip()
    if not chunk: continue
    try: items = json.loads(chunk)
    except Exception: continue
    if isinstance(items, dict): items = items.get("workitems") or []
    for it in items or []:
        wid = it.get("id")
        if wid in seen: continue
        seen.add(wid)
        desc = it.get("description") or ""
        status = "?"
        for ln in desc.splitlines():
            if "<!-- STATUS:" in ln:
                status = ln.strip(); break
        cat_name = it.get("categoryName") or it.get("category") or ""
        subject = it.get("subject","")
        print("\t".join([str(wid), str(cat_name), subject, status]))
'
    ;;
  get)
    id="$1"
    echo "-----BEGIN UNTRUSTED CONTENT-----"
    $API "/oapi/v1/projex/organizations/{orgId}/workitems/$id"
    echo "-----END UNTRUSTED CONTENT-----"
    ;;
  comments)
    id="$1"
    echo "-----BEGIN UNTRUSTED CONTENT-----"
    $API "/oapi/v1/projex/organizations/{orgId}/workitems/$id/comments"
    echo "-----END UNTRUSTED CONTENT-----"
    ;;
  comment)
    id="$1"; text="$2"
    body=$(python3 -c "import json,sys;print(json.dumps({'content': sys.argv[1]}))" "$text")
    $API "/oapi/v1/projex/organizations/{orgId}/workitems/$id/comments" -X POST --body "$body"
    ;;
  create-topic|create-bug)
    title="$1"; body_file="$2"
    [[ -f "$body_file" ]] || { echo "body_file not found: $body_file" >&2; exit 2; }
    description=$(cat "$body_file")
    if [[ "$cmd" == "create-topic" ]]; then TYPE_ID="$TASK_TYPE"; else TYPE_ID="$BUG_TYPE"; fi
    subject="$PREFIX $title"
    body=$(python3 -c "
import json, sys
print(json.dumps({
    'spaceId': '$SPACE_ID',
    'workitemTypeId': '$TYPE_ID',
    'subject': sys.argv[1],
    'assignedTo': '$ASSIGNEE',
    'description': sys.argv[2]
}))
" "$subject" "$description")
    echo "-----BEGIN UNTRUSTED CONTENT-----"
    $API "/oapi/v1/projex/organizations/{orgId}/workitems" -X POST --body "$body"
    echo "-----END UNTRUSTED CONTENT-----"
    ;;
  body-set)
    id="$1"; body_file="$2"
    [[ -f "$body_file" ]] || { echo "body_file not found: $body_file" >&2; exit 2; }
    description=$(cat "$body_file")
    body=$(python3 -c "import json,sys;print(json.dumps({'description': sys.argv[1]}))" "$description")
    $API "/oapi/v1/projex/organizations/{orgId}/workitems/$id" -X PUT --body "$body"
    ;;
  status-line)
    id="$1"
    $API "/oapi/v1/projex/organizations/{orgId}/workitems/$id" \
      | python3 -c "
import sys, json
d=json.load(sys.stdin); desc=d.get('description') or ''
for ln in desc.splitlines():
    if '<!-- STATUS:' in ln: print(ln.strip()); break
else: print('<!-- STATUS:UNKNOWN -->')
"
    ;;
  last-updated)
    id="$1"
    $API "/oapi/v1/projex/organizations/{orgId}/workitems/$id" \
      | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('gmtModified') or d.get('gmtCreate'))"
    ;;
  status-set)
    # yx.sh status-set <id> <discussing|implementing|reviewing|done>
    # 同步更新云效原生 status 字段（Boss 在云效 UI 能看到）
    # 同时建议配合 body-set 更新 body 里的 <!-- STATUS --> 标记
    id="$1"; phase="$2"
    case "$phase" in
      discussing|consensus|spec_ready) native="100005" ;;  # 待处理
      implementing)                     native="142838" ;;  # 开发中
      reviewing)                        native="100012" ;;  # 测试中
      done)                             native="100014" ;;  # 已完成
      *) echo "unknown phase: $phase (expected: discussing|implementing|reviewing|done)" >&2; exit 2 ;;
    esac
    body=$(python3 -c "import json,sys;print(json.dumps({'status': sys.argv[1]}))" "$native")
    $API "/oapi/v1/projex/organizations/{orgId}/workitems/$id" -X PUT --body "$body"
    ;;
  *)
    echo "Unknown command: $cmd"
    echo "Available: list | get | comments | comment | create-topic | create-bug | body-set | status-line | last-updated"
    exit 2
    ;;
esac
