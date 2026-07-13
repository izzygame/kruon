# kruon W1 运行核心与真实闭环实施计划

## 目标

完成 W1 技术门：Rust 本地运行核心、SQLite 事件回放、进程树治理、路径策略、Codex/Claude 真实只读探针、ADR 与用户访谈材料。

## 分工

- Codex：Git 基线、任务编排、代码审查、真实探针、最终验收。
- Mimo：仅安装并验证 Rust stable 工具链。
- OpenCode：实现 `apps/desktop/src-tauri/src/core/`、Tauri 命令、Rust 测试与闭环脚本。
- Hermes：使用 Coding Plan `deepseek-v4-flash-260425` 审查安全与一致性，并补齐 ADR；不覆盖 OpenCode 核心实现。

## 阶段

| 阶段 | 状态 | 产出 |
|---|---|---|
| 1. Git 安全基线 | in_progress | 忽略项/敏感项审查、本地基线提交 |
| 2. Rust 工具链 | pending | rustup/rustc/cargo、Tauri cargo check |
| 3. OpenCode 运行核心 | pending | ProcessSupervisor、EventStore、路径策略、适配器 Host、Tauri API |
| 4. Codex 联合审查 | pending | 编译、测试、安全与行为修正 |
| 5. 双适配器真实探针 | pending | 各一次受控只读执行、脱敏 fixtures、回放证据 |
| 6. Hermes ADR 审查 | pending | ADR-001~005、失败模式审查与修订 |
| 7. 研究材料与 W1 验收 | pending | 访谈包、端到端演示、完整验证报告 |

## 已锁定决策

- 产品适配器保留 Codex + Claude；Claude 不承担开发协作。
- 真实探针仅使用临时无敏感夹具目录；每个适配器一次、60 秒超时，Claude 费用上限 0.10 美元。
- 本地初始化 Git，不推送远程；代理顺序工作，Codex 统一审查。
- W1 内不扩展大规模 UI，也不声称已实现网络出口隔离。

## 验收标准

- 既有 177 项测试不回退，新增 Rust 测试与 `cargo check` 通过。
- SQLite 追加、幂等、冲突、回放与非终态恢复行为可验证。
- 取消能治理进程组；强停后检查残留，不能确认时进入 `uncertain`。
- 越界、路径穿越和符号链接逃逸在进程启动前拒绝。
- Codex 与 Claude 真实事件能归一化、持久化并回放；探针目录无文件变更。
- ADR、访谈脚本、授权说明和录屏模板齐备。

## 错误记录

| 错误 | 尝试 | 处理 |
|---|---|---|
| 先前 rustup/Homebrew 安装均超时 | 2 | 网络已恢复；由 Mimo 只重试一次官方 rustup，失败即转 Codex 诊断 |
| 敏感项扫描命令中的混合引号触发 zsh 解析错误 | 1 | 拆分为不含嵌套引号的文件名扫描与高置信模式扫描 |
