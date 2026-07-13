# kruon W1 S1-03/S1-06 发现

- 既有 adapter spike 已包含冻结 LaunchPlan、危险参数拒绝、规范事件 schema、Codex/Claude 能力快照和 60 个单元测试。
- 当前缺口是显式 Adapter 方法契约与可机器校验的 capability manifest schema；快照目前只由 dataclass 宽松加载。
- 产品仍以 Codex/Claude 为目标适配器；本轮将 Claude Code 替换为 Hermes 仅指开发协作执行器，不擅自改变产品范围。
- Workspace/Policy 威胁模型尚未形成独立文档。
- S1-03 已形成 12 方法同步/异步 Adapter 契约与 capability manifest schema；`ArtifactCandidate.in_workspace` 改为 fail-closed 默认值。
- 适配器测试 99/99、合成 fixture 10/10、领域状态机 26/26、桌面 UI 3/3 均通过；类型检查和前端构建通过。
- 合成 fixture 只能证明解析与契约稳定，不能证明两款 CLI 的真实任务、审批、取消或恢复闭环。
- S1-06 威胁模型已纠正证据等级：SQLite 重建、真实超时、UI 差异表达和双 CLI 真实任务仍未验证。
- 当前本机缺少 Rust/Cargo。Mimo 的官方 rustup 下载与 Homebrew 镜像更新均超时，已清理全部残留安装进程。
