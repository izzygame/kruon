# kruon W1 S1-03/S1-06 实施计划

## 目标

在不启动真实高风险 Agent Run 的前提下，完成正式 Adapter 最小契约、capability schema 与 Workspace/Policy 威胁模型 v1，为后续 Codex/Claude 深适配 spike 设置可验证边界。

## 分工与边界

- OpenCode：仅修改 `spikes/cli-adapters/`，把既有能力快照升级为可校验的 S1-03 契约；不得执行真实模型调用，不得修改应用、规划或产品文档。
- Hermes（Coding Plan / `deepseek-v4-flash-260425`）：仅创建 `docs/security/workspace-policy-threat-model-v1.md`；基于产品计划、现有 adapter spike 和本机边界做独立威胁审查。
- Mimo：本任务暂不参与架构判断；后续仅承担版本/help 采集或依赖安装。
- Codex：任务拆分、越界检查、schema/威胁模型主审、测试和下一决策门。

## 阶段

| 阶段 | 状态 | 产出 |
|---|---|---|
| 1. 现状与接口缺口确认 | complete | 既有 Python spike、事件 schema、能力快照审查 |
| 2. OpenCode S1-03 | complete | Adapter Protocol、capability schema、快照校验与测试 |
| 3. Hermes S1-06 | complete | Workspace/Policy 威胁模型 v1 |
| 4. Codex 联合审查 | complete | 安全默认、证据等级、Python 3.9 兼容性与测试收口 |
| 5. 下一决策门 | complete | 下一批先执行 S1-04 Codex 服务协议 spike；S1-05 不调用 Claude 作为开发协作者 |

## 验收标准

- Adapter 共性接口覆盖 probe/capabilities/prepare/start/sendInput/streamEvents/respondApproval/cancel/resume/collectArtifacts/reconcile/diagnostics。
- capability schema 必须明确 `approval_mode`、证据等级、版本、未知/不支持，且 Codex 不能被误标为 `per_action`。
- 快照通过 schema 校验；危险 bypass 参数继续被拒绝；原 60 个测试不得回退。
- 威胁模型至少覆盖路径越界、符号链接与竞态、命令/参数注入、网络外传、secret/日志、工作区信任、审批绑定、进程树和恢复不确定性。
- 所有结论区分已验证、推断与未验证；未知终态不得改写为成功。

## 错误记录

| 错误 | 尝试 | 处理 |
|---|---|---|
| `dataclass(kw_only=True)` 不兼容系统 Python 3.9 | 1 | 改为必填字段在前、默认字段在后的 3.9 兼容 dataclass；99 项测试通过 |
| Mimo 通过 rustup/Homebrew 安装 Rust 均遇下载超时 | 2 | 终止并清理残留 Homebrew 进程；Rust/Cargo 仍为环境阻塞，仓库未被修改 |
