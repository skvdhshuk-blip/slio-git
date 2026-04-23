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
