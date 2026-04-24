## Context

`commit-changed-files-panel` 已经让历史视图支持“提交详情 + 变动文件列表”，但当前点击文件后的实现仍然沿用主工作区 diff 流程：`HistoryMessage::ViewCommitFileDiff` 在 `main.rs` 里加载提交文件 diff，然后调用 `show_history_commit_file_diff(...)`，直接切到 `ShellSection::Changes` 和 `GitToolWindowTab::Changes`，并覆写全局 `current_diff`、`selected_change_path` 与 `diff_source`。  

这条路径的问题不是 diff 能力本身，而是上下文被整体挪走：用户原本在 Git Log 中按提交逐个检查文件，一次点击就离开历史界面。项目当前也没有真正的多原生窗口架构，现有“独立窗”交互主要通过 app-level overlay / popup 完成，因此本次更适合做“历史上下文里的独立 diff 弹出窗”，而不是引入新的窗口生命周期系统。

## Goals / Non-Goals

**Goals:**
- 点击历史视图中的提交文件时，弹出专用 diff 窗口，不切换到主工作区
- 弹出窗关闭后，历史视图的提交选中、文件选中和滚动上下文保持不变
- 复用现有 diff 视图能力与样式，确保历史提交文件 diff 的展示与主 diff 体验一致
- 隔离历史弹出窗状态，避免污染当前工作区的文件选择、diff 来源和编辑器状态

**Non-Goals:**
- 不引入真正的多原生窗口管理、窗口拖拽/停靠/独立生命周期
- 不在历史提交 diff 弹出窗中支持 stage / unstage / revert 等工作区动作
- 不改造历史视图的提交文件列表来源或提交范围计算逻辑
- 不把单文件弹出窗扩展成整提交多文件分页浏览器

## Decisions

1. **使用 app-level modal 弹出窗，而不是切换 shell 或创建新的 OS window**  
   选择在 `AppState` 顶层增加历史 diff 弹出窗状态，并在 `view()` 最上层渲染 modal overlay。  
   原因：现有应用是单窗口 Iced 壳层，branch popup、pending dialog 等都已经使用 app-level overlay；这条路径能直接满足“弹出一个单独的窗”的交互目标，同时避免引入额外窗口同步、关闭事件和跨窗口状态共享复杂度。  
   备选方案：
   - 继续复用 `AuxiliaryView`：仍然会替换整个主体区域，本质上还是跳转
   - 新建原生窗口：技术可行但超出这次特性需求，且会扩大状态同步范围

2. **为历史提交文件 diff 建立独立状态，而不是复用当前工作区 diff 状态**  
   新增独立的 popup state（包含 `commit_id`、`file_path`、`Diff`、presentation、hunk 选中和 editor runtime state）。  
   原因：当前 `show_history_commit_file_diff(...)` 直接写入 `selected_change_path`、`current_diff` 和 `diff_source`，这是导致跳转和状态污染的根源。独立状态可以让历史 diff 的打开、关闭、切换文件都保持局部化。  
   备选方案：
   - 继续复用全局 diff 字段并“打开后再恢复”：恢复点太多，容易遗漏并引入新的上下文 bug
   - 只存 `Diff` 不存运行时状态：会丢失 presentation / hunk / editor 滚动等交互状态

3. **抽取共享的 diff surface，而不是在弹出窗里复制一份 header/content 逻辑**  
   将当前主工作区使用的 diff header、content、导航逻辑整理成可复用的共享构建入口，然后主工作区与历史弹出窗都基于同一套渲染逻辑。  
   原因：用户要求“功能和现在的差异视图一样”，如果在弹出窗里重新写一套简化 viewer，很快会和主工作区产生行为漂移。  
   备选方案：
   - 复制 `build_diff_header` / `build_diff_content`：短期快，但后续维护会分叉
   - 弹出窗只放 text diff：无法满足与现有 diff 视图一致的目标

4. **为历史提交 diff 增加“从已加载 Diff 构建只读 editor state”的能力**  
   现有 unified editor 已支持 `UnifiedDiffEditorState::from_diff(...)`，但 split diff editor 仍依赖工作区文件路径与 `build_editor_diff_model(...)`。为了让弹出窗尽量具备与当前 diff 面板一致的表现，需要补齐一个“从 `Diff` 直接生成只读 split model/runtime state”的入口，供历史提交文件 diff 复用。  
   原因：如果不补这一层，历史弹出窗只能退回到统一视图，无法真正达到与当前 diff 视图一致。  
   备选方案：
   - 在历史弹出窗中禁用 split：实现简单，但和用户目标不符
   - 仍然把历史 diff 映射成工作区路径再构建 split：语义不对，且对 deleted/renamed/root commit 场景脆弱

5. **弹出窗保持只读，并以“关闭返回历史”作为主流程**  
   弹出窗内保留文件标题、差异统计、presentation 切换、hunk 导航与滚动，但不显示工作区相关动作。关闭方式包括显式关闭按钮和 scrim dismiss。  
   原因：历史提交 diff 的核心是审查，不是修改；只读边界越清晰，越不容易把工作区行为错误地带进提交历史审查流程。  

## Risks / Trade-offs

- [历史弹出窗状态与主工作区 diff 状态并存，容易出现双份逻辑] → 通过抽取共享 diff surface 和统一的 reset/helper 逻辑，减少重复分支
- [从 `Diff` 直接构建 split model 可能在 rename / root commit / deleted file 上出现边界差异] → 为这些场景补充单元测试和 UI smoke 校验
- [app-level overlay 可能和已有弹层发生层级冲突] → 在 `view()` 顶层明确历史 diff popup 的 z-order，并约束其关闭行为优先级
- [大文件或大 diff 在弹出窗中初始化 editor state 较重] → 仅在真正打开弹出窗时懒加载 runtime state，关闭后立即释放

## Migration Plan

- 无数据迁移或持久化结构迁移
- 用历史弹出窗入口替换当前 `show_history_commit_file_diff(...)` 的导航行为
- 保留现有 diff 加载函数 `load_history_commit_file_diff(...)`，仅调整消费方式
- 如实现出现严重回归，可回退到当前“切换到主工作区 diff”的旧路径

## Open Questions

- 当前默认关闭策略是否需要支持 `Esc` 快捷键；若本轮不做，可保留显式关闭按钮 + scrim 点击关闭作为最小可用方案
