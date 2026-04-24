## Why

IDEA 的 Git Log 视图中，选中一个提交后右侧/下方会显示该提交的变动文件列表（树形目录结构 + 文件变更类型图标）。slio-git 当前的历史视图只显示提交信息（作者、时间、message），没有变动文件面板，用户无法直观查看某个提交改了哪些文件。

## What Changes

- 在历史视图中选中提交后，右侧提交详情区域新增"变动文件"面板
- 调用 `git2` 比较该提交与其父提交的 tree diff，获取变动文件列表
- 支持平铺和树形两种展示模式（与变更列表的 Flat/Tree 模式一致）
- 点击变动文件可查看该提交的文件 diff（只读）

## Capabilities

### New Capabilities

### Modified Capabilities

## Impact

- `src/git-core/src/commit.rs` 或 `diff.rs` — 新增 `get_commit_changed_files(repo, commit_id)` 函数
- `src-ui/src/views/history_view.rs` — 提交详情区域新增变动文件面板
- `src-ui/src/state.rs` — `HistoryState` 新增 `selected_commit_files` 字段
