# kruon W1 S1-04/DEV-006 实施计划

## 目标

在不执行真实模型任务的前提下，确定本机 Codex `exec`、`app-server`、`exec-server` 的可观察协议边界，并形成进程树取消/隔离的实现决策输入。

## 分工与边界

- OpenCode：仅修改 `spikes/codex-service-protocol/**`；允许执行 Codex 版本、帮助、schema/类型生成等只读协议发现命令；禁止发起真实模型调用，禁止修改其他目录。
- Hermes（Coding Plan / `deepseek-v4-flash-260425`）：仅创建 `docs/security/macos-process-isolation-options-v1.md`；评估进程组取消、残留检测、文件系统/网络隔离的候选方案和验证清单，不把未验证方案写成既定实现。
- Mimo：暂不重试 Rust 安装；仅在下载链路恢复后承担机械安装。
- Codex：主审协议证据、复核本机命令、运行测试并作 S1-04 决策。

## 阶段

| 阶段 | 状态 | 产出 |
|---|---|---|
| 1. 任务拆分与边界冻结 | complete | 本计划 |
| 2. OpenCode 协议发现 | complete | 原始 help/schema/TS 证据、分析器、49 项测试与决策草案 |
| 3. Hermes 隔离方案审查 | complete | macOS 进程与隔离选项说明，DeepSeek V4 Flash 使用记录 |
| 4. Codex 联合审查 | complete | 纠正 exec/app-server 能力混淆、取消语义和 O_NOFOLLOW 可行性 |
| 5. S1-04 决策 | complete | 当前实现基线为 sandbox_policy_only；app-server per_action 仅为待闭环验证候选 |

## 验收标准

- 保存本机 Codex 版本与服务命令的原始、可重采集证据。
- 明确 `exec --json`、`app-server`、`exec-server` 是否暴露逐动作审批、会话、取消、恢复和产物协议；没有证据时必须标记 unknown/unverified。
- 探针不得发起真实模型请求，不得使用危险 bypass 参数。
- macOS 方案说明必须把进程组治理、TOCTOU、文件系统隔离、网络隔离分开评估。
- 决策必须能回写 capability snapshot，且不会把服务存在误写成协议已稳定。

## 错误记录

| 错误 | 尝试 | 处理 |
|---|---|---|
| OpenCode 初稿把根 CLI 审批参数误当作 exec 能力，并把 app-server 写成 PTY | 1 | 退回修订；按 root/exec/app-server/exec-server 原始证据分层，改为 JSON-RPC 候选 |
| OpenCode 初稿把 schema 声明直接推为 per_action 决策 | 1 | 主审降级为 protocol_declare，所有 closed_loop 保持 false |
| Hermes 初稿把 O_NOFOLLOW 当作 Adapter Host 可直接实施防线 | 1 | 修订为仅适用于受控文件代理/系统隔离，移出 W1 |
| OpenAI Codex 手册助手因代理响应缺少校验头失败 | 1 | 添加官方文档 MCP；当前任务仍以本机 0.144.2 原始 help 与生成 schema 为版本级证据 |
