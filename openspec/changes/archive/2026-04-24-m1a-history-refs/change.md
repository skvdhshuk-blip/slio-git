# M1-A PR1: History Refs Backfill + branches_containing_commit

## Summary
git-core 数据回灌层实现（不含 UI）。对应 Discussion #3 Wave 1。

## Files Changed
- `src/git-core/src/history.rs` — `entry_from_commit` 接受 ref_map 参数，所有 get_history* 函数调用 `compute_ref_labels` 回灌 `refs`（含 HEAD 标志位）
- `src/git-core/src/branch.rs` — 新增 `BranchRef` struct 和 `branches_containing_commit` API
- `src/git-core/src/lib.rs` — 导出 `BranchRef`, `branches_containing_commit`
- `src/git-core/tests/history_refs_tests.rs` — fixture 单测（新文件）

## Deferred
- Signature 读取 → M1-G
- UI 消费（ref chip / Details refs 区）→ M1-A PR2

---

# M1-A PR2: History UI — Row Refs Badge + HEAD Chip + Details Refs/Contained in Branches

## Summary
消费 PR1 已 backfill 的 `HistoryEntry.refs`，在 log row 渲染 refs 徽章 + HEAD chip，Details 面板追加 Refs 小节 + Contained in Branches 小节。

## Files Changed

### S1 — Row refs 徽章 + HEAD chip
- `src-ui/src/views/history_view.rs` — 新增 `build_ref_chips(entry: &HistoryEntry) -> Option<Element<HistoryMessage>>` 辅助函数；`build_commit_row` 在 hash 之后插入 `chips_row`（`Length::Shrink`）；subject 改为 `Wrapping::None`
- `src-ui/src/i18n.rs` — 新增 `commit_refs_label` / `contained_in_branches_label` / `loading_containing_branches` / `no_containing_branches` 4 条 label（zh+en+ZH_CN static+EN static）

### S2 — Details Refs + Contained in Branches 面板
- `src-ui/src/views/history_view.rs` — `HistoryState` 新增 3 字段（`selected_commit_containing_branches` / `_loading` / `_error`）；`select_commit` 同步调用 `branches_containing_commit` 填充；新增 `build_refs_chiplist_panel(entry, i18n) -> Option<Element>` 和 `build_containing_branches_panel(state, i18n) -> Element`；`build_commit_detail` 接入两个新面板

### Tests
- `src-ui/src/views/history_view.rs::tests` — 4 个新单测：`build_ref_chips_yields_head_local_remote_tag_order` / `build_ref_chips_empty_when_refs_empty` / `containing_branches_panel_groups_local_remote` / `refs_panel_hides_when_empty`

## Constraints Met
- `signature_status` token: 0 出现在 diff 中
- 净增 ~296 行（≤500）
- `cargo test -p git-core -p src-ui` 全绿
