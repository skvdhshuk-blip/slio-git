## 1. 历史 diff 弹出窗状态与消息流

- [x] 1.1 在 `src-ui/src/state.rs` 为历史提交文件差异新增独立 popup state，并补齐打开、关闭、重置所需的 helper
- [x] 1.2 调整 `src-ui/src/main.rs` 中 `HistoryMessage::ViewCommitFileDiff` / `show_history_commit_file_diff(...)` 的处理逻辑，改为写入 popup state 而不是切换到 `ShellSection::Changes`
- [x] 1.3 保持 `HistoryState.selected_commit`、`selected_commit_file_path` 和当前 Log tab 上下文在弹出窗打开/关闭前后一致

## 2. 复用现有差异视图能力

- [x] 2.1 从现有主工作区 diff 头部、内容区和导航逻辑中抽取共享的只读 diff surface 构建入口，供主界面与历史弹出窗共同使用
- [x] 2.2 为历史提交文件差异补齐基于已加载 `Diff` 的 editor/runtime state 构建逻辑，使弹出窗支持与现有差异视图一致的 presentation 和 hunk 导航体验

## 3. 弹出窗 UI 集成

- [x] 3.1 新增历史提交文件差异弹出窗视图，包含标题、文件信息、关闭入口、scrim 与只读 diff 内容区
- [x] 3.2 在 app-level `view()` 中接入历史 diff 弹出窗 overlay，并确保它覆盖历史视图而不改变当前 shell section 或 tool-window tab
- [x] 3.3 支持在历史视图中重复点击不同文件时刷新弹出窗内容，而不是退出历史上下文重新进入主工作区

## 4. 验证

- [x] 4.1 为历史 diff 弹出窗补充状态/交互测试，覆盖“点击文件不跳转主界面”“关闭后上下文保持不变”“切换 presentation 仍留在历史视图”
- [ ] 4.2 运行 `cargo test --manifest-path src-ui/Cargo.toml`，并做一次历史视图手工 smoke check：打开提交、点击文件、查看差异、关闭弹出窗、重复打开另一文件
