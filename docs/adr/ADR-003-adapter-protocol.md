# ADR-003：固定双适配器协议与进程组治理

- 状态：已采纳
- 日期：2026-07-14

## 背景

Codex 与 Claude 输出协议不同，但产品层必须使用同一 Run/Event 模型，且调用方不能传入任意命令。

## 决策

`StartRunRequest` 只接收适配器、工作区、提示、超时和策略引用。Rust 内部生成固定启动计划：

- Codex：`codex exec --json --sandbox read-only --ephemeral`。
- Claude：`--output-format stream-json --verbose --permission-mode plan --no-session-persistence --no-chrome --max-budget-usd 0.10`。

提示通过 stdin 输入，不进入命令行参数。子进程建立独立进程组；取消先 SIGTERM，10 秒后仍未结束则标记 `forced_stop_required` 并 SIGKILL。无法确认进程组清理时终态为 `uncertain`。取消请求一旦建立，即使子进程退出码为 0，也按 `cancelled` 收口。

## 后果

- 两种协议被归一化为同一 `EventEnvelope`，未知或畸形行以降级事件和哈希记录。
- 产品 API 不具备任意命令执行能力。
- Unix 进程组无法阻止恶意子进程主动 `setsid` 脱离；W1 只声明尽力治理，不声明容器级隔离。
- `uncertain` 时仍存活的输出读取线程不能强制 join；运行时保留管理对象，后续版本需加入受控 reaper。

## 复审条件

CLI 协议版本变化、需要 Windows Job Object，或残留进程证据显示进程组不足时复审。
