## Why

历史视图已经支持在提交详情里展示“变动文件”列表，但点击文件后会调用现有主工作区 diff 流程，直接跳回 Changes 主界面。这个跳转会打断用户在 Git Log 中浏览提交和文件的上下文，也让“只想快速看某个提交里的单文件差异”变成一次全局视图切换。

## What Changes

- 为历史视图中的“变动文件”点击行为新增专用的提交文件差异弹出窗，而不是切回主界面的 Changes/diff 工作区
- 弹出窗内复用当前 diff 视图的核心能力，让用户继续查看统一/分栏差异、文件标题、hunk 导航和只读提交差异内容
- 保持历史视图的提交选中状态、文件列表选中状态和滚动上下文；关闭弹出窗后回到原来的历史视图位置
- 为历史提交文件差异引入独立状态，避免污染当前工作区文件选择和未提交改动 diff 状态

## Capabilities

### New Capabilities
- `history-commit-diff-popup`: 在历史视图中以独立弹出窗查看某个提交下单个文件的差异，并保持 Git Log 浏览上下文不变

### Modified Capabilities

## Impact

- `src-ui/src/main.rs` — 历史文件 diff 的消息路由、弹出窗打开/关闭、diff 状态切换逻辑
- `src-ui/src/state.rs` — 新增历史提交文件差异弹出窗的顶层状态，隔离主工作区 diff 状态
- `src-ui/src/views/history_view.rs` — 变动文件点击交互和弹出窗触发入口保持在历史上下文中
- `src-ui/src/views/` 或现有 diff 相关视图/组件 — 复用并适配现有 diff viewer/editor 到只读历史提交弹出窗场景
