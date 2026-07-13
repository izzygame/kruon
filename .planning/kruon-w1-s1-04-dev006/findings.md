# kruon W1 S1-04/DEV-006 发现

- S1-03 暂将 Codex `exec --json` 定义为 `sandbox_policy_only`；是否存在稳定逐动作通道仍需服务协议证据。
- 真实模型调用不是本任务的必要条件；优先使用帮助文本、内建 schema/类型生成和本地协议握手。
- Rust/Cargo 尚未安装，不阻止独立 Python/文档 spike，但阻止原生 Tauri 集成验证。
- 本机 Codex CLI 为 0.144.2。`codex exec --help` 不含逐动作审批参数，因此该表面只能诚实声明 `sandbox_policy_only`。
- app-server 可生成 82 个 v1 与 516 个 v2 schema definitions，并声明 6 类审批消息；这只证明 `protocol_declare`，未证明真实双向闭环。
- app-server 是 JSON-RPC over stdio/Unix socket/WebSocket，不是 PTY。exec-server 同样标记为 experimental。
- `CommandExecTerminateParams` 仅声明终止 app-server `command/exec` 进程，不能推导整个 Run、进程树或残留清理能力。
- S1-04 当前实现基线为 `codex exec --json --sandbox workspace-write`；这不是生产安全证明，也不能在 UI 中展示为逐动作审批。
- Hermes 复核确认：kruon 不能给上游 CLI 自己的 `open()` 调用附加 O_NOFOLLOW；W1 只能做事后检测、终止后续动作并标记失败/unknown。
- 进程组、残留检测、macOS Sandbox 与网络出口都保留 `spike_needed`；网络暂缓需要产品负责人显式接受剩余风险。
- 最终回归：adapter 99、Codex protocol 49、domain 26、desktop 3，共 177 项通过；fixture 10/10、类型检查和前端构建通过。
