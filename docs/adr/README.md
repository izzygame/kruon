# Kruon 架构决策记录

| ADR | 决策 | 状态 |
|---|---|---|
| [ADR-001](ADR-001-technology-stack.md) | Tauri 2 + Rust 本地核心 + Web 前端 | 已采纳 |
| [ADR-002](ADR-002-run-event-model.md) | Run 投影与追加式 Event 日志 | 已采纳 |
| [ADR-003](ADR-003-adapter-protocol.md) | 固定双适配器协议与进程组治理 | 已采纳 |
| [ADR-004](ADR-004-security-boundary.md) | 工作区、环境、存储和 IPC 安全边界 | 已采纳 |
| [ADR-005](ADR-005-codex-integration.md) | Codex 只读、临时、参数受控集成 | 已采纳 |
| [ADR-006](ADR-006-local-first-credential-and-local-model-policy.md) | local-first、凭据保管与本地模型离线语义 | 已采纳 |
| [ADR-007](ADR-007-world-view-event-projection-and-capability-isolation.md) | 2D/3D 同源投影与世界窗口能力隔离 | 已采纳 |
| [ADR-008](ADR-008-alpha-cli-version-compatibility-gate.md) | Alpha CLI 精确版本矩阵与启动前 fail-closed 门 | 已采纳 |
| [ADR-009](ADR-009-alpha-fault-containment-and-migration-atomicity.md) | Alpha 故障收敛、输出预算与数据库迁移原子性 | 已采纳 |
| [ADR-010](ADR-010-metadata-only-diagnostic-export.md) | 元数据白名单诊断包、二次隐私扫描与原子导出 | 已采纳 |
| [ADR-011](ADR-011-macos-alpha-packaging-and-data-lifecycle.md) | macOS Alpha 打包、签名公证门与本地数据生命周期 | 已采纳（外部门开放） |
| [ADR-012](ADR-012-derived-onboarding-and-recovery-guidance.md) | 派生式首次连接进度、幂等只读示例与错误码恢复引导 | 已采纳 |
| [ADR-013](ADR-013-alpha-security-preflight-and-external-review-gate.md) | Alpha 安全预审、本地存储加固与独立评审门 | 已采纳（外部评审开放） |

这些决策覆盖 W1-M4 当前技术闭环。每项都列出当前未解决的边界；“已采纳”不等于对应风险已经被操作系统级沙箱完全消除。
