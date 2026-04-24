# Changelist DnD Stage/Unstage — Proposal

## Problem

The changelist widget (`src-ui/src/widgets/changelist.rs`) supports +/- buttons and hunk-level staging, but lacks drag-and-drop between Staged and Unstaged sections. IDEA Changes Tab uses DnD as a primary muscle-memory operation; its absence makes the tool feel like a demo.

## Motivation

011 US1 Acceptance Criterion #2 explicitly requires DnD between Staged/Unstaged groups. T020 was left `[ ]`.

## Scope (this PR)

- File-level DnD: drag a file row from Unstaged → Staged header (stages it), or Staged → Unstaged header (unstages it)
- 4px drag threshold protects single-click selection
- Ghost label follows cursor during drag (Stack overlay)
- Section header highlights on hover during drag
- Esc cancels drag

## Non-scope (future topics)

- Hunk-level DnD (existing StageHunk/UnstageHunk buttons already cover granular workflow)
- Multi-select DnD (requires `selected_change_path: HashSet<String>`, independent topic)
- Window-exit detection (on_exit → hover_kind=None → release cancel)
