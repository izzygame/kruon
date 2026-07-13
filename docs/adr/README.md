# Kruon 架构决策记录

| ADR | 决策 | 状态 |
|---|---|---|
| [ADR-001](ADR-001-technology-stack.md) | Tauri 2 + Rust 本地核心 + Web 前端 | 已采纳 |
| [ADR-002](ADR-002-run-event-model.md) | Run 投影与追加式 Event 日志 | 已采纳 |
| [ADR-003](ADR-003-adapter-protocol.md) | 固定双适配器协议与进程组治理 | 已采纳 |
| [ADR-004](ADR-004-security-boundary.md) | 工作区、环境、存储和 IPC 安全边界 | 已采纳 |
| [ADR-005](ADR-005-codex-integration.md) | Codex 只读、临时、参数受控集成 | 已采纳 |

这些决策覆盖 W1 技术闭环。每项都列出当前未解决的边界；“已采纳”不等于对应风险已经被操作系统级沙箱完全消除。
