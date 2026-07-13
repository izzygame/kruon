# ADR-001：Tauri 2 + Rust 本地核心

- 状态：已采纳
- 日期：2026-07-14

## 背景

Kruon 需要桌面 UI、本地 CLI 编排、SQLite 持久化和 macOS 进程树控制。W1 的重点是可验证的本地执行闭环，而不是扩展界面。

## 决策

保留 Tauri 2 与现有 Web 前端；把运行、事件、路径和进程治理放在 Rust 核心中。SQLite 使用 `rusqlite` bundled，避免依赖用户机器上的 SQLite 版本。前端只通过固定 Tauri 命令访问领域接口。

## 后果

- Rust 可直接使用 Unix 进程组、信号和强类型领域模型。
- 单个桌面包同时承载 UI 与本地核心，部署面较小。
- CI 必须同时执行 Node 检查以及 Rust fmt、check、test。
- 当前仅验证 macOS/aarch64；Windows 进程树语义需要独立实现。

## 未选择方案

- Electron：生态成熟，但 W1 不需要额外 Node 主进程与更大的运行面。
- 独立本地 daemon：隔离更强，但增加安装、升级和认证复杂度。
- 全部使用 TypeScript：难以直接、可靠地覆盖进程组和信号语义。

## 复审条件

需要多用户 daemon、跨机器调度，或 Tauri 平台能力阻塞核心需求时复审。
