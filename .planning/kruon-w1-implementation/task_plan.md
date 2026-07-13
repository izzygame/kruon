# kruon W1 实施计划

## 目标

启动 W1 技术预研，先交付可复现的双 CLI 能力探针、规范事件 schema、fixture 测试骨架与审批能力差异记录，为后续核心领域模型和 Tauri 工程骨架提供真实契约依据。

## 分工

- Codex：主导任务边界、架构审查、结果验证与后续核心领域模型。
- Claude Code：只在 `spikes/cli-adapters/` 实现 S1-04/S1-05 的能力探针与 fixture 骨架。

## 阶段

| 阶段 | 状态 | 产出 |
|---|---|---|
| 1. 委派与边界冻结 | complete | Claude 任务说明、允许目录、验收标准 |
| 2. Claude 实施 | complete | 探针、schema、fixtures、测试和说明 |
| 3. Codex 审查 | complete | diff、测试、安全与事实核验 |
| 4. 修正与验收 | complete | 可复现结果、已知限制、下一任务包 |

## 验收标准

- 变更仅位于 `spikes/cli-adapters/`。
- 不调用任何危险绕过权限参数，不读取或输出 token。
- 探针默认只执行 `--version` / `--help` 等无副作用命令；真实模型调用必须显式 opt-in。
- schema 明确表达 `approval_mode = per_action | sandbox_policy_only | none`。
- 至少包含成功、畸形 JSON、未知事件、非零退出和取消/终态 fixture 测试。
- README 给出可复现命令、事实与推断的区分、未解决风险。

## 错误记录

| 错误 | 尝试 | 处理 |
|---|---|---|
| Claude CLI 启动后进入长任务会话，首轮未直接返回结果 | 1 | 保留会话 93875 并轮询，不重复派发 |
| Claude 实施达到 3 美元预算上限后停止 | 1 | 保留已完成产物；先审查缺口，再拆成只补 fixtures、测试和 README 的小任务 |
| Claude 续作再次达到 1.5 美元预算上限 | 2 | 测试已落盘；停止继续委派，由 Codex 接管验证、修复和 README 收尾 |
| 通用完成检查脚本未识别 scoped plan，显示 0/0 | 1 | 以当前 scoped task_plan 的 4/4 complete 和实际测试结果为准 |
