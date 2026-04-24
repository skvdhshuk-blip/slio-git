## Context

slio-git 的历史视图 (`history_view.rs`) 已有提交列表和提交详情面板。详情面板显示 commit message、作者、时间、parent IDs。缺少 IDEA 风格的变动文件列表。

git-core 已有 `diff_refs()` 函数可以比较两个 ref 的差异，返回 `Diff` 结构。可以用它比较 `commit^..commit` 获取变动文件。

## Goals / Non-Goals

**Goals:**
- 选中提交时自动加载变动文件列表
- 支持 Flat（路径列表）和 Tree（目录树）两种展示
- 文件名旁显示变更类型标记（A=新增, M=修改, D=删除, R=重命名）
- 点击文件查看该提交对该文件的 diff

**Non-Goals:**
- 不做文件内容的 blame/annotate
- 不做多提交范围的文件变动聚合
- 不做文件的 inline diff 预览（先跳到 diff 全屏视图）

## Decisions

1. **数据层**: 新增 `git_core::commit::get_commit_changed_files(repo, commit_id) -> Vec<ChangedFile>`，内部用 `repo.diff_tree_to_tree(parent_tree, commit_tree)` 获取
2. **状态**: `HistoryState` 新增 `selected_commit_files: Vec<ChangedFile>` 和 `selected_commit_file_display: FileDisplayMode`
3. **UI**: 变动文件面板放在提交详情下方，复用 `changelist` widget 的渲染逻辑（Flat/Tree 模式切换按钮）
4. **加载时机**: 在 `SelectCommit` handler 中同步加载（提交的文件列表通常不大，不需要异步）

## Risks / Trade-offs

- merge 提交有多个 parent，默认比较第一个 parent（与 IDEA 行为一致）
- 大提交（100+ 文件）可能导致列表长，用 scrollable 限高
