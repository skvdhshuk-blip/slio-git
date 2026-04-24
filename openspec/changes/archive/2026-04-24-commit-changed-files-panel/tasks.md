## 1. 数据层

- [x] 1.1 在 `src/git-core/src/commit.rs` 新增 `CommitChangedFile` 结构体（path, old_path, status: Added/Modified/Deleted/Renamed）
- [x] 1.2 实现 `get_commit_changed_files(repo, commit_id) -> Result<Vec<CommitChangedFile>>` — 用 `diff_tree_to_tree(parent_tree, commit_tree)` 获取变动
- [x] 1.3 处理 root commit（无 parent 时与空 tree 比较）和 merge commit（取第一个 parent）
- [x] 1.4 在 `lib.rs` 导出新类型和函数

## 2. 状态层

- [x] 2.1 `HistoryState` 新增 `selected_commit_files: Vec<CommitChangedFile>` 字段
- [x] 2.2 在 `HistoryMessage::SelectCommit` handler 中加载变动文件列表
- [x] 2.3 选中提交变化时清空旧的文件列表

## 3. UI 层

- [x] 3.1 在 `history_view.rs` 的提交详情面板下方新增变动文件面板
- [x] 3.2 渲染文件列表 — 每行显示变更类型标记（绿A/蓝M/红D/紫R）+ 文件路径
- [x] 3.3 添加 Flat/Tree 模式切换按钮（复用 `FileDisplayMode`）
- [x] 3.4 Tree 模式下按目录分组渲染（复用 `tree_widget` 逻辑）
- [x] 3.5 显示文件数统计（如 "2 个文件"）

## 4. 文件 Diff 查看

- [x] 4.1 新增 `HistoryMessage::ViewCommitFileDiff(commit_id, file_path)` 消息
- [x] 4.2 点击文件时调用 `diff_refs` 获取该文件在该提交的 diff，显示在 diff 视图中

## 5. 验证

- [x] 5.1 `cargo build --release` 编译通过
- [x] 5.2 为 `get_commit_changed_files` 编写单元测试
