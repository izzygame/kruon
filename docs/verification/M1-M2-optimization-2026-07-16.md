# M1/M2 复盘与优化实施

日期：2026-07-16

## 现场问题

从资源管理器启动桌面程序后，控制台将 Codex 和 Claude 都显示为未找到。现场检查结果：

- Codex 已安装：`C:\Users\Izzy\AppData\Roaming\npm\codex.cmd`，版本 `codex-cli 0.144.1`。
- Claude 当前没有可发现的本机可执行文件。

因此问题不是两个适配器都未安装，而是 M1 的探测和实际启动均只依赖进程继承的 `PATH`。桌面程序从资源管理器启动时可能继承旧的环境变量，导致用户 npm bin 不在 `PATH`；即便探测侥幸成功，原先的实际 Run 仍会重新用裸命令名启动，存在探测与执行不一致。

## M1 优化

- 新增统一适配器解析：先检查 `PATH`，再检查常见用户级目录，包括 `%APPDATA%\npm`、`%LOCALAPPDATA%\pnpm`、`%USERPROFILE%\.local\bin`、`%USERPROFILE%\.cargo\bin` 与 Node 标准安装目录。
- Windows 支持 `.exe`、`.cmd`、`.bat`；npm 安装的 `codex.cmd` 因而可以被直接解析。
- 探测和实际 `LaunchPlan` 使用同一解析结果的绝对路径，消除了“面板显示可用但 Run 启动失败”或相反的分叉。
- 受控子进程环境会保留白名单变量，并补入解析到的 CLI 父目录和 Node 运行时目录；不放行 API Key、SSH 代理或其它敏感环境变量。
- Tool connections 卡片现在显示解析/探测详情。Claude 未安装时会明确说明检查范围，而不是只给出抽象的 `not_found`。

## M2 复盘

M2 的“冻结策略、Artifact、完成报告、人工接受/退回、审计”闭环保持有效，且 M2 之前已补上刷新时回读持久化验收结论。当前仍应保留以下发布门：

- 20 组真实 Codex/Claude 任务的启动率与终态率没有样本；
- Claude 尚未安装，也尚未验证生产级逐动作审批回传；
- 因此当前产品只声明已验证的只读、`sandbox_policy_only` 路径，不声明逐动作批准或会话续接。

## 验证

```text
codex.cmd --version（最小 PATH）            PASS: codex-cli 0.144.1
pnpm --filter @kruon/desktop typecheck       PASS
pnpm --filter @kruon/desktop test            PASS: 5/5
pnpm --filter @kruon/desktop build           PASS
cargo fmt --check                            PASS
cargo test --all-targets                     PASS: 核心 26/26，w1_probe 2/2
```

新增 Rust 回归覆盖 Windows npm `.cmd` shim 解析、绝对 shim 在受控环境中的探测，以及受控执行环境包含解析到的程序目录。

## 后续操作

重新构建后的桌面程序会找到当前已安装的 Codex。Claude 在安装其 CLI 并重新打开程序后，使用同一解析机制自动发现；安装和登录属于用户凭据/外部软件变更，本次未执行。

## 运行期回归：Codex 版本探测

优化版程序实际运行后，连接卡片显示 `version_check_failed`，详情为“resolved via PATH; version probe did not complete”。该问题已复现并定位为探测命令与新版 Codex CLI 的交互约束不匹配：在 Kruon 的安全子进程环境中，标准输入明确设为 `null`；`codex --version` 会因此以退出码 1 返回 `Error: stdin is not a terminal`。这不是 Codex 安装、路径解析或登录状态错误。

`codex exec --version` 是同一 CLI 的非交互版本入口，在相同无标准输入条件下返回 `codex-cli-exec 0.144.1`，因此 M1 改为仅对 Codex 使用 `exec --version`；Claude 仍使用其顶层 `--version`。Windows 命令 shim 的回归测试同时断言了这一参数契约，避免后续改回交互入口。

首次重建的现场复验仍失败，进一步确认 GUI 进程的 PATH 解析顺序也会影响结果：Codex Desktop 的打包资源可能在用户 npm shim 之前被命中。解析器现优先检查约定的 per-user CLI 目录（包括 `%APPDATA%\\npm`），再回退到 PATH；对应回归测试覆盖了“用户 `codex.cmd` 优先于 PATH 中同名资源”的约束。这样连接面板与实际 Run 启动始终使用已验证的用户 CLI。

第二次现场探测表明已经解析到该 per-user shim，但 Node 版 shim 会继续从 PATH 查找 `node`。受控环境现在把解析到的 CLI 目录和已验证的 Node 运行时目录置于 PATH 最前，并补齐 Windows GUI 进程常用的 `USERPROFILE`、`TEMP`、`TMP` 白名单变量。回归测试断言 CLI 父目录位于该受控 PATH 的首位。

如果此类探测仍失败，连接面板不再以笼统的“did not complete”掩盖原因：它只会返回安全分类（终端要求、Node 运行时、不可执行、超时或通用失败）及退出码，绝不回显 CLI 的原始 stderr、环境变量或配置内容。这使运行期诊断可验证，同时维持受控日志的脱敏边界。

现场安全诊断最终显示为“version probe timed out”，而非退出码失败。原先 3 秒上限不足以覆盖此机器上 Codex CLI 的桌面进程冷启动；现改为有界的 15 秒。该值只影响连接面板中的只读 `--version` 探测，不改变任务运行超时、执行权限或任何模型调用策略。
