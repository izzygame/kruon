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
| 1. Git 安全基线 | complete | 忽略项/敏感项审查、本地基线提交 `2a4f27d` |
| 2. Rust 工具链 | complete | rustup 1.29.0、rustc/cargo 1.97.0、aarch64；基线编译到项目宏 |
| 3. OpenCode 运行核心 | complete | 独立工作树提交 `7095569`；OpenCode 搭骨架，Codex 接管并完成核心 |
| 4. Codex 联合审查 | in_progress | 编译、测试、安全与行为修正 |
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
| `mimo run` 不接受 `--never-ask-questions` 子命令参数 | 1 | 该参数属于根命令；改为 `mimo --never-ask-questions run ...`，不启用危险跳过权限参数 |
| Mimo 官方 rustup 安装在其 120 秒工具上限内仅下载约 6.3/11 MB | 1 | 按计划停止 Mimo 重试；Codex 改用官方直链、可续传下载并延长超时后安装 |
| 校验后的安装器以 `kruon-rustup-init` 文件名运行，被 rustup 多调用代理机制误判 | 1 | 保留已校验二进制，恢复官方期望文件名 `rustup-init` 后运行 |
| OpenCode 在 Rust 主安装尚未结束时违背指令并发执行 `rustup update`，留下孤儿安装进程 | 1 | 立即终止 OpenCode 会话及孤儿 rustup；确认其工作树零源码修改，只保留官方主安装进程，工具链稳定后重新派发 |
| 初次 Tauri `cargo check` 在 `generate_context!` 因缺少 `icons/icon.png` 失败 | 1 | 工具链和依赖编译均成功；把现有 Tauri 图标配置缺口纳入 OpenCode crate 范围修复并回归 |
| 运行时取消集成测试在进程已清理后挂起 | 1 | 线程采样确认 wait 线程持有 Child mutex 后递归进入取消收口；缩短锁作用域后再进入 finalize |
| OpenCode 连续三次生成超长且无法解析的 EventStore 写入请求 | 3 | 按三次失败协议终止会话；保留已完成领域骨架，由 Codex 用小补丁完成实现和测试 |
| 领域 Node 测试误用 Vitest 运行，26 个 node:test 子测试通过但 Vitest 报无 suite | 1 | 改用仓库原生 `node --test`，26/26 正式通过 |
