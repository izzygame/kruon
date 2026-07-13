# kruon MVP 开发规划发现

## 已确认输入

- `kruon_MVP产品需求说明书_2026-07-11.docx` 已通过用户审查。
- 当前工作区没有应用源码或工程配置，只有 PRD 生成脚本、品牌资料、脑暴材料与规划记录。
- PRD 定义 10 周邀请制 Alpha，P0 核心为本地任务控制闭环。

## 待验证

- Tauri 2 + React + SQLite 是否作为首选工程骨架。
- Codex/Claude 两款 CLI 的稳定事件、审批、取消、恢复与产物契约。
- 并行 Run 的隔离策略：默认 worktree、可选目录隔离或串行回退。
- 3D 技术栈与 Tauri WebView 的性能预算。
- macOS 签名、公证、更新与本地权限提示的交付路径。

## 交叉审查与本机复核

- Claude Code CLI 只读审查指出双适配器审批能力可能不对称；随后已用本机 `codex --help`、`codex exec --help` 与 `codex app-server --help` 复核。
- 已确认：顶层交互式 Codex 有 `--ask-for-approval`，但 `codex exec` 非交互模式没有该参数；`exec` 只有 sandbox、危险绕过、JSONL 输出等能力。
- 已确认：Codex 存在实验性的 `app-server`、`exec-server` 与 remote-control 路径，但是否提供可供 kruon 使用的逐动作审批协议仍需 W1 spike。
- 已确认：`codex exec` 有 `--ignore-user-config`，可用于降低用户配置中危险绕过或策略漂移的影响；关键配置仍需显式覆盖和回归。
- 因此适配器契约必须声明 `approval_mode = per_action | sandbox_policy_only | none`，不能把两款工具伪装为审批能力等价。
- W1 首要决策：Codex 选择实验性 app-server/exec-server、PTY 交互适配，还是诚实降级为前置沙箱策略。该选择决定 FR-06 的实际实现边界。

## Claude Code 协作边界

- 适合拆分：CLI 能力矩阵、安全威胁清单、测试 fixture 设计、独立方案审查、文档校对。
- 不直接委派：核心领域模型、权限语义、状态机、数据库迁移策略、最终架构决策。
