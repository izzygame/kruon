# kruon W1 运行核心实施发现

- 当前目录尚未初始化 Git，但已有 `.gitignore`，覆盖 node_modules、dist、target、环境文件、日志与常见缓存。
- Tauri Rust crate 目前仅有最小启动壳，尚无原生核心模块或 SQLite 依赖。
- 本机 OpenCode 1.17.18、Hermes 0.18.2、Mimo 0.1.0、Claude Code 2.1.205、Codex 0.144.2 可用；cargo/rustc 缺失。
- 既有 Python adapter spike 已提供统一事件 schema、Codex/Claude synthetic fixtures 和安全 opt-in 探针。
- 上一阶段累计 177 项自动化检查通过，可作为回归基线。
- 真实 Claude 探针可使用 plan 权限模式、stream-json、禁用会话持久化与费用上限；Codex 可使用 read-only sandbox 与 ephemeral 会话。

