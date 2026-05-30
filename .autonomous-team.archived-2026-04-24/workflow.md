# Workflow Definition — Autonomous Team v2.0

## Vision

成为取代 IntelliJ IDEA 内置 Git 工具窗口的全功能 Git GUI —— 让用户日常 Git 操作完全无需打开 IDEA，仍然获得同等（或更好）的体验。

## Decision Constitution

按优先级从高到低，冲突时上位条目胜出：

1. **IDEA 功能对齐优先** — 任何设计决策，先对齐 `~/git/idea` 中 Git 工具窗口的行为（Log/Changes/Branch/Commit/Diff/Push/Merge/Rebase/Stash/History/Annotate 等）。偏离需有明确的 UX/技术理由并在 Spec 里说明。
2. **坚持现有技术栈** — Rust 2024 + Iced 0.14 + git2 0.19 + notify 8 + syntect + tokio。不更换 UI 框架、不换 Git 绑定、不换 runtime。
3. **非必要不引入新依赖** — 新依赖必须经 Technical Architect 与 Cost Analyst 在 Discussion 中给出不可替代性论证；标准库 / 已存在依赖优先。
4. **原生体验与稳定性** — macOS 原生手感（快捷键、字体、右键菜单、拖拽）与 "绝不丢用户提交/修改" 的稳定性，优先于 nice-to-have 功能。
5. **增量与可验证** — 功能以小步切片交付，每 PR ≤ 500 行；每个 Spec 必须有可在 IDEA 对照复现的验收剧本。

## Hard Constraints

- **IDEA 行为作为验收基线** — 涉及已在 IDEA 存在的功能，Spec 必须给出 "在 IDEA 中的对照操作路径 + 期望一致点"，由 Acceptance Tester 按此复核。
- **技术栈锁定** — 不得引入新的 UI 框架 / Git 库 / 异步运行时；替换或升级 iced / git2 主版本必须走 HEAVY Discussion。
- **依赖守门** — PR 中新增 `[dependencies]` 条目需在 Discussion 中显式列出候选与否决理由，默认拒绝。
- **无 unsafe 代码** — `src/` 下禁止 `unsafe`，除非在 git2 FFI 边界且经 Security Reviewer 审阅。
- **cargo 红线** — `cargo build`、`cargo test`、`cargo clippy -- -D warnings` 三件套在 CI/本地均须通过，PR 合并前全绿。
- **GitHub as Single State Source** — 所有状态存活在 GitHub（Discussion STATUS、PR labels），不使用本地状态文件。
- **Spec Driven** — 所有功能实现必须先在 Discussion body 内冻结 Spec。
- **Granularity Control** — 单 PR ≤ 500 行，避免 context 撑爆。
- **Mission Driven** — 团队围绕 Vision 推进，而不是被动消费队列。
- **Event Driven** — 任何 agent 都不得 sleep/blocking wait，事件驱动 + 心跳兜底。

## Milestone Goals

- **M1 — Git Log 对齐**：提交图 / 分支过滤 / 作者过滤 / 文件过滤 / 右键 Checkout/Revert/Cherry-pick，与 IDEA Log 面板视觉与交互一致。
- **M2 — Commit & Diff 对齐**：变更文件面板（changelist、prev/current/new）、双栏 diff、语法高亮、stage/unstage、inline revert。
- **M3 — Branches & Remote 对齐**：分支弹窗（search + recent + local + remote，按 IDEA 1:1 布局）、push / pull / fetch / track、PR 外部开关。
- **M4 — Merge / Rebase / Stash / Cherry-pick**：含冲突解决器（三栏 merge）、stash list、rebase 进度。
- **M5 — 可用性**：file history、annotate / blame、search in log、settings、keymap 对齐、崩溃自愈。

---

## Architecture

### Two Persistent Agents

| Role | Who | Responsibilities |
|------|-----|------------------|
| Team Lead | Main agent (runs /loop) | Team health, GitHub scanning, Boss interaction, env support, **sole agent spawner** |
| Facilitator | Persistent opus agent | Full topic lifecycle: Discussion → Consensus → Spec → Implement → Review → Merge (no direct spawn) |

### Dynamic Team Members (spawned by Team Lead, SendMessage communication, context isolated)

| Role | Phase | Spawn Trigger | Terminate When |
|------|-------|---------------|----------------|
| Technical Architect | Discussion | Facilitator → SPAWN_REQUEST → Team Lead | Consensus reached |
| Perspective Roles (1-2) | Discussion | Facilitator → SPAWN_REQUEST → Team Lead | Consensus reached |
| Mission Analyst | Mission Review | Facilitator → SPAWN_REQUEST → Team Lead | Analysis complete |
| Executor | Implementation | Facilitator → SPAWN_REQUEST → Team Lead | PR merged |
| Code Reviewer | Review | Facilitator → SPAWN_REQUEST → Team Lead | Review done |
| Acceptance Tester | Review | Facilitator → SPAWN_REQUEST → Team Lead | Review done |
| Security Reviewer | Review | Facilitator → SPAWN_REQUEST → Team Lead | Review done |

All dynamic members are spawned by Team Lead as background team members (`run_in_background: true`). Facilitator never spawns directly — requests via SendMessage.

### Perspective Selection

| Topic Attribute | Additional Perspective(s) |
|----------------|--------------------------|
| User-facing features | + Product Owner |
| Security-sensitive | + Security Expert |
| Performance/scalability | + Performance Expert |
| Cost/resource | + Cost Analyst |
| Multiple attributes | + Multiple perspectives |
| Default (no match) | + Product Owner |

Technical Architect is always included for HEAVY topics. Minimum: TA + 1 additional.

### Two-Round Discussion Protocol

Round 1 (Perspectives): Each role posts domain-specific viewpoint independently.
Synthesis: Facilitator combines all perspectives, identifies agreements and tensions.
Round 2 (Challenge): All roles read synthesis, challenge cross-domain issues.
Max 1 additional round if challenges raised, then finalize.

---

## State Management

### GitHub is the Only State Source

**No local state files.** No `discussion-queue.md`, no `task-index.md`.

Every Discussion body starts with a machine-readable status line:

```
<!-- STATUS:{phase} [PR:#{N}] [SINCE:{ISO8601}] -->
```

| Status | Meaning |
|--------|---------|
| DISCUSSING | Discussion active, waiting for perspectives |
| CONSENSUS | Consensus reached, Spec being written |
| SPEC_READY | Spec frozen in body, ready for implementation |
| IMPLEMENTING | Executor working, PR number attached |
| REVIEWING | PR under review |
| DONE | PR merged, topic complete |

### State Reconstruction (5 commands)

Any agent can rebuild global state at any time:
```bash
gh api repos/{owner}/{repo}/discussions                    # All Discussions + STATUS
gh pr list --state open --json number,title,labels         # Open PRs
gh pr view {N} --json labels                               # Review labels
git worktree list                                          # Active Executors
git status                                                 # Local git state
```

---

## Daily Workflow

### Task Type Classification

| Type | Prefix/Label | Discussion | Consensus | Spec | Reviewers |
|------|-------------|------------|-----------|------|-----------|
| HEAVY | `[Feature]` or complex | ✅ TA + Perspectives | ✅ Two-round | ✅ Full | 3 (all) |
| MEDIUM | `[Small]` or simple | ✅ TA only | ✅ Single-round | ✅ Full | 3 (all) |
| LIGHT | `[Bug]` or Issue with `bug` label | ❌ Skip | ❌ Skip | ❌ Skip | 3 (all) |
| DOC | `[Doc]` | ❌ Skip | ❌ Skip | ❌ Skip | 1 (Code Reviewer only) |
| REVIEW | `[Mission Review]` | ✅ Analyst + Perspectives | ✅ Two-round | ❌ Output = topic list | N/A |

### Workflow Summary

Detailed phase logic is defined in `facilitator.md`. High-level flow:

```
HEAVY:   Discussion (TA + Perspectives, two-round) → Consensus → Spec → Executor → 3 Review → Merge
MEDIUM:  Discussion (TA only) → Confirm → Spec → Executor → 3 Review → Merge
LIGHT:   Executor → 3 Review → Merge (skip discussion)
DOC:     Executor → 1 Review (Code Reviewer only) → Merge
REVIEW:  Discussion (Analyst + Perspectives, two-round) → Consensus → Output: topic list
```

Team Lead provides heartbeat every 10 min for timeout enforcement and liveness detection.

### Bug Fast Track (LIGHT)

```
Team Lead detects Issue with "bug" label →
  Team Lead: Add "team-tracked" label
  Team Lead: Notify Facilitator
  Facilitator: Create Discussion (STATUS:IMPLEMENTING, skip Phase 1-2)
  → Phase 3: Facilitator → SPAWN_REQUEST → Team Lead: Executor (PR body: "Fixes #{issue}")
  → Phase 4: Facilitator → SPAWN_REQUEST → Team Lead: 3 Reviewers
  → Phase 5: Merge → Issue auto-closed
```

### Mission Review

```
Trigger: Queue empty OR every N completed topics

Facilitator: Create Discussion "[Mission Review] 阶段回顾"
  Body: completed topic list + Constitution reference
Facilitator: → SPAWN_REQUEST → Team Lead: Mission Analyst + Perspective Role
  → Analyst: Analyze codebase vs mission gap, propose topics
  → Perspective Role: Review and challenge priorities
  → Boss: Can comment (high-priority perspective)
  → Consensus = next topic list
  → Facilitator: Create new Discussions for each proposed topic
```

---

## Decision Autonomy Rules

### Team Decides Independently (No Boss Approval)

| Question | Method | Decision Maker |
|----------|--------|----------------|
| Should feature be built? | Discuss in Discussion | Discussion consensus |
| Option A or B? | Use Constitution priority | Discussion consensus |
| Fix bug or not? | Affects core path? | Technical Architect |
| PR granularity? | > 500 lines → must split | Technical Architect |
| Review pass? | All required labels present | Facilitator |
| Next topic? | Priority order | Facilitator |

### Team Lead Boundary

- ✅ Team health (spawn, terminate, health check)
- ✅ GitHub scanning (new Discussions, Issues, label changes)
- ✅ Boss interaction (needs-boss Issues, closure detection)
- ✅ Environment support (install dependencies, or escalate)
- ✅ Knowledge base maintenance
- ❌ NOT solve project problems
- ❌ NOT review code
- ❌ NOT write specs or make project decisions

### Boss Participation

- Boss can comment directly in any Discussion
- Boss comments are treated as high-priority perspective
- Boss can override direction decisions via Discussion comments
- Boss creates Discussions/Issues for new work
- Boss closes needs-boss Issues to provide resources
- Boss reviews mission checkpoints (optional)

---

## Role Division

### Project-Level Roles (Persistent)

| Role | Model | Scope | Key Responsibilities |
|------|-------|-------|---------------------|
| Team Lead | (env) | All | Health check, GitHub scan, Boss interaction, env, spawn agents |
| Facilitator | opus | All | Queue, Discussion lifecycle, consensus, Spec, merge, mission review |

### Discussion-Level Roles (Dynamic, per Discussion)

| Role | Model | Phase | Key Responsibility |
|------|-------|-------|-------------------|
| Technical Architect | opus | Discussion | Technical proposal |
| Product Owner | opus | Discussion | User value + challenge |
| Security Expert | opus | Discussion | Security + challenge |
| Performance Expert | opus | Discussion | Performance + challenge |
| Cost Analyst | sonnet | Discussion | Cost + challenge |
| Mission Analyst | opus | Mission Review | Gap analysis + topic proposal |
| Executor | sonnet | Implementation | Code + PR |
| Code Reviewer | sonnet | Review | Code quality + label |
| Acceptance Tester | sonnet | Review | Feature validation + label |
| Security Reviewer | sonnet | Review | Security audit + label |

---

## Event-Driven + Heartbeat Pattern

### Principle

**No agent sleeps or blocks.** All agents follow:
1. Receive message → check state → act → return to idle
2. Team Lead heartbeat (10 min) provides timeout enforcement

### Timeout Enforcement

| Phase | Timeout | Who Checks | Enforcement |
|-------|---------|-----------|-------------|
| Discussion consensus | 30 min | Facilitator (on heartbeat) | TIMEOUT-PROCEED |
| Confirmation round | 10 min | Facilitator (on heartbeat) | Finalize with available |
| PR review | 30 min | Facilitator (on heartbeat) | Prompt reviewer or proceed |

### Liveness Detection

| Target | Method | Fallback |
|--------|--------|----------|
| Facilitator | TaskOutput(task_id, block=false) + idle notification + heartbeat response | TaskStop + respawn |
| Dynamic agents | State inspection (expected output missing on GitHub) | Re-spawn |

---

## Required Responsibilities Checklist

| Responsibility | Owner |
|----------------|-------|
| `topic_collect` | Facilitator |
| `topic_schedule` | Facilitator |
| `flow_advancement` | Facilitator |
| `discussion_create` | Facilitator |
| `consensus_facilitate` | Facilitator |
| `spec_write` | Facilitator |
| `perspective_post` | Dynamic Perspective Roles (TA, Product Owner, etc.) |
| `technical_solution` | Technical Architect (dynamic) |
| `spec_freeze` | Facilitator |
| `code_implement` | Executor (dynamic) |
| `code_review` | Code Reviewer (dynamic) |
| `feature_accept` | Acceptance Tester (dynamic) |
| `security_review` | Security Reviewer (dynamic) |
| `pr_merge` | Facilitator |
| `executor_cleanup` | Facilitator (terminate after merge) |
| `env_support` | Team Lead |
| `perspective_spawn` | Facilitator (request) → Team Lead (execute) |
| `github_scan` | Team Lead |
| `boss_resource` | Team Lead |
| `mission_analysis` | Mission Analyst (dynamic) |
| `heartbeat` | Team Lead |
| `liveness_check` | Team Lead |

---

## Review Label System

GitHub prevents self-review. Use labels as approval gates:

| Reviewer | Pass Label | Fail Label |
|----------|-----------|------------|
| Code Reviewer | `code-review-passed` | `code-review-needs-fix` |
| Acceptance Tester | `acceptance-passed` | `acceptance-failed` |
| Security Reviewer | `security-passed` | `security-issue` |

Each reviewer adds their label + checks if all 3 present → notifies Facilitator.

DOC type: only `code-review-passed` required.

---

## Configuration

Stored in `.autonomous-team/config.json`:

```json
{
  "language": "{language}",
  "boss_github_username": "{username}",
  "version": "2.0.0",
  "mission_checkpoint_interval": 5
}
```

All dynamic agents receive `language` setting in spawn prompt.
