# Boss Interaction Guide

> All Boss interaction happens via GitHub. No local files needed.

## Boss Actions

| Action | How | Team Response |
|--------|-----|---------------|
| Request feature | Create GitHub Discussion | Team Lead detects → Facilitator queues |
| Report bug | Create GitHub Issue (label: `bug`) | Team Lead detects → Facilitator fast tracks |
| Participate in discussion | Comment on Discussion | Facilitator treats as high-priority perspective |
| Provide resource | Close `needs-boss` Issue with comment | Team Lead notifies requesting agent |
| Review progress | Read Discussion STATUS lines | N/A |
| Adjust direction | Comment on [Mission Review] Discussion | Facilitator incorporates into next topic list |

## Team → Boss Requests

When team needs external resources (API keys, permissions, paid tools):
1. Team Lead creates GitHub Issue with label `needs-boss`
2. Boss checks filter: `label:needs-boss is:open`
3. Boss closes Issue with resolution comment
4. Team Lead detects closure → notifies agent

## Topic Priority

| Source | Default Priority |
|--------|-----------------|
| Boss-created Discussion | P1 (high) |
| Boss-created Issue (bug) | P1 (high, fast track) |
| Team-proposed (from Mission Review) | P3 (low, Boss topics always override) |
