# Changelist DnD Stage/Unstage — Design

## State Machine

```
Idle
  --on_press(path, kind)--> Armed { path, src_kind, anchor=cursor, started=false }
Armed
  --on_move |p-anchor| < 4px--> Armed (cursor updated)
  --on_move |p-anchor| >= 4px--> Dragging (started=true)
  --on_release--> Idle (click, no drag dispatch)
Dragging
  --on_move(p)--> Dragging (cursor updated)
  --on_enter(section=B)--> Dragging (hover_kind=B)
  --on_release, hover_kind != src_kind--> Stage/UnstageFile; Idle
  --on_release, hover_kind == src_kind or None--> Idle (cancel)
  --Esc--> Idle (cancel)
```

## Event Flow

1. User presses file row → outer `mouse_area.on_press` fires `BeginFileDrag(path, kind)`
2. Cursor tracking via existing `TrackChangeContextMenuCursor` also calls `drag.update_cursor()`
3. When `dist >= 4px`, `drag.started = true` → ghost renders in Stack overlay
4. Moving over section header triggers `mouse_area.on_enter` → `DragHoverSection(kind)`
5. Release → `DragRelease` → if `started && hover_kind != src_kind` → call stage/unstage

## iced Constraints

- iced 0.14 has no native DnD API; synthesized from `mouse_area` press/move/release
- Ghost Container must NOT have any handler (`on_enter`/`on_press`) to avoid swallowing drop target events
- Button outer `mouse_area.on_press` captures drag start; Button's own `on_press` fires for select (iced dispatches outer→inner)
- `Stack` second layer is the ghost; `drag.started` gates its visibility

## Files Changed

| File | Change |
|---|---|
| `src-ui/src/state.rs` | `ChangeSectionKind` pub enum; `DragState` struct + methods; `AppState.drag` field |
| `src-ui/src/widgets/changelist.rs` | `SectionKind` internal enum; 4 new handlers; ghost Stack overlay; drop target header highlight |
| `src-ui/src/main.rs` | 4 new Messages; update branches; Esc subscription; cursor tracking wires drag.update_cursor |
