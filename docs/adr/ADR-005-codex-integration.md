# ADR-005：Codex 只读、临时、参数受控集成

- 状态：已采纳
- 日期：2026-07-14

## 背景

Codex 是 W1 的产品适配器之一。目标是证明协议和恢复闭环，不是开放通用代理执行。

## 决策

Codex 仅由固定适配器以 JSON、read-only sandbox、ephemeral 会话运行；工作目录必须通过路径策略。真实探针使用独立临时目录和最小 README，60 秒超时，只执行一次。事件归一化后再保存，原始非 JSON stderr 不落库。

策略引用与提示哈希写入 Run。当前 Rust 核心尚未把 Python 协议实验中的逐动作 approval fingerprint 接入产品路径，因此 W1 不宣称已经具备 Codex 工具级审批闭环。

## 证据

`codex-cli 0.144.2` 的受控探针完成，生成 40 个归一化事件，Run 终态为 `completed`，夹具 SHA-256 前后相同。脱敏夹具见 `spikes/cli-adapters/fixtures/live/`。

## 后果

- 可基于真实协议做回归，而不把 Kruon 仓库交给探针读取。
- 临时和只读参数降低持久化与写入风险，但不等同于网络隔离。
- Codex CLI 升级后必须先用合成夹具回归；真实探针需要新的显式执行门。

## 复审条件

增加工具写入、用户审批、会话恢复或 Codex 协议版本变化时复审。
