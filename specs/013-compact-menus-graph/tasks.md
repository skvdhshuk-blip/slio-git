# Tasks: 紧凑右键菜单和提交图

**Input**: Design documents from `/specs/013-compact-menus-graph/`
**Prerequisites**: plan.md, spec.md
**Tests**: SC-003 requires zero regression against existing 94 tests.
**Organization**: Tasks grouped by user story for independent implementation.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)

---

## Phase 0: Setup

**Purpose**: Confirm baseline — all existing tests pass before any changes

- [x] T001 Run `cargo test --workspace` to establish baseline test count (expect 94 tests passing) in project root — Completed in T3 PR #1 87f552e

**Checkpoint**: Baseline green — safe to proceed with constant adjustments

---

## Phase 1: User Story 1 — 紧凑右键菜单 (P1)

**Goal**: Reduce menu item vertical padding from 10px to 4px, row height from ~34px to ~24px, hide detail text, match IDEA compact density

**Independent Test**: Right-click any commit → menu items compact, no subtitle descriptions, all 12 items visible without scrolling

### Implementation

- [x] T002 [US1] Reduce `action_row` padding from `[10, 8]` to `[4, 8]` in src-ui/src/widgets/menu.rs (FR-001) — Completed in T3 PR #1 87f552e
- [x] T003 [P] [US1] Reduce menu group padding from `[8, 10]` to `[4, 8]` in src-ui/src/widgets/menu.rs (FR-001) — Completed in T3 PR #1 87f552e
- [x] T004 [P] [US1] Remove detail/description text rendering from menu item rows — hide subtitle text entirely in src-ui/src/widgets/menu.rs (FR-002) — Completed in T3 PR #1 87f552e
- [x] T005 [P] [US1] Set group header font size to 10px and separator height to 1px with 2px vertical spacing in src-ui/src/widgets/menu.rs (FR-003) — Completed in T3 PR #1 87f552e
- [x] T006 [P] [US1] Reduce `HISTORY_CONTEXT_MENU_WIDTH` constant from `332.0` to `280.0` in src-ui/src/views/history_view.rs (FR-008) — Completed in T3 PR #1 87f552e
- [x] T007 [P] [US1] Reduce branch popup context menu item padding from `[8, 12]` to `[4, 8]` in src-ui/src/views/branch_popup.rs (FR-009) — Completed in T3 PR #1 87f552e

**Checkpoint**: All context menus (commit log + branch popup) display at compact IDEA density, SC-001 satisfied

---

## Phase 2: User Story 2 — 紧凑提交图 (P1)

**Goal**: Reduce commit row height to 22px, graph lane width to 14px, node radius to 3px, line width to 1.5px

**Independent Test**: Open log view → commit rows visibly denser, ≥10% more rows visible per screen vs baseline

### Implementation

- [x] T008 [US2] Reduce `HISTORY_ROW_HEIGHT` from `24.0` to `22.0` in src-ui/src/views/history_view.rs (FR-004) — Completed in T3 PR #1 87f552e
- [x] T009 [P] [US2] Reduce `HISTORY_GRAPH_LANE_WIDTH` from `16.0` to `14.0` in src-ui/src/views/history_view.rs (FR-005) — Completed in T3 PR #1 87f552e
- [x] T010 [P] [US2] Reduce `HISTORY_GRAPH_NODE_RADIUS` from `4.0` to `3.0` in src-ui/src/views/history_view.rs (FR-006) — Completed in T3 PR #1 87f552e
- [x] T011 [P] [US2] Reduce `HISTORY_GRAPH_LINE_WIDTH` from `1.6` to `1.5` in src-ui/src/views/history_view.rs (FR-006) — Completed in T3 PR #1 87f552e
- [x] T012 [P] [US2] Reduce commit row padding from `[8, 10]` to `[4, 8]` in src-ui/src/views/history_view.rs (FR-007) — Completed in T3 PR #1 87f552e

**Checkpoint**: Log view row density matches IDEA visual reference, SC-002 satisfied (≥10% more rows per screen)

---

## Phase 3: Polish

**Purpose**: Verify all changes together and confirm zero regressions

- [x] T013 Run `cargo test --workspace --locked` and confirm all 94 existing tests still pass (SC-003) — Completed in T3 PR #1 87f552e
- [x] T014 [P] Run `cargo clippy --workspace -- -D warnings` and fix any new warnings introduced — Completed in T3 PR #1 87f552e
- [x] T015 [P] Run `cargo fmt --all -- --check` and fix any formatting issues — Completed in T3 PR #1 87f552e

**Checkpoint**: SC-003 satisfied — zero test regression across all 3 modified files

---

## Dependencies

- 012-idea-ui-refactor must be merged (menu structure and graph rendering code must be present)
- No new git-core changes required (UI-only constant adjustments)

## Parallel Execution Examples

- T002 through T007 can all run in parallel (all different files or non-overlapping sections of menu.rs)
- T008 through T012 can all run in parallel (same file but different constants — trivial merge)
- T013 through T015 must run after T002–T012 complete

## Implementation Strategy

All changes are numeric constant adjustments across 3 files (`menu.rs`, `history_view.rs`, `branch_popup.rs`). No logic changes, no new functions, no new types. The Plan's Change Map table lists every constant with its before/after value — use it as a checklist. Each task maps directly to one constant or one layout property. Risk of regression is minimal since only visual density parameters change.
