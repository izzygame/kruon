# kruon W1 运行核心实施发现

- 当前目录尚未初始化 Git，但已有 `.gitignore`，覆盖 node_modules、dist、target、环境文件、日志与常见缓存。
- Tauri Rust crate 目前仅有最小启动壳，尚无原生核心模块或 SQLite 依赖。
- 本机 OpenCode 1.17.18、Hermes 0.18.2、Mimo 0.1.0、Claude Code 2.1.205、Codex 0.144.2 可用；cargo/rustc 缺失。
- 既有 Python adapter spike 已提供统一事件 schema、Codex/Claude synthetic fixtures 和安全 opt-in 探针。
- 上一阶段累计 177 项自动化检查通过，可作为回归基线。
- 真实 Claude 探针可使用 plan 权限模式、stream-json、禁用会话持久化与费用上限；Codex 可使用 read-only sandbox 与 ephemeral 会话。
- OpenCode 首次实现会话在工具链安装期间越过“不要再运行 rustup”的明确边界；会话已终止且工作树仍干净。后续必须在 Rust 安装完成后重新开新会话，并把工具链操作设为禁止项。
- Rust stable 已安装为 1.97.0（2026-07-07），rustup 1.29.0，host 为 `aarch64-apple-darwin`。
- 基线 `cargo check` 已完成 crates.io 依赖下载和 Tauri 依赖编译，唯一项目级错误是仓库原本缺少 Tauri 默认 `icons/icon.png`。
- OpenCode 第二次会话成功写入 Tauri 配置、依赖和领域骨架，但其 DeepSeek 工具写入在 EventStore 大文件上连续失败；Codex 接管后完成全部核心。
- Rust 核心当前 21 项测试通过：19 个库测试与 2 个探针守卫测试；另有 `cargo check --all-targets`、rustfmt、合成探针与项目路径拒绝验证。
- 运行时取消集成测试曾发现真实 Child mutex 重入死锁，已通过线程采样定位并修复；正常/强停/超时/重复取消测试均通过。
- 全仓回归：adapter 99、Codex protocol 49、domain 26、desktop 3 全部通过，10/10 fixtures、类型检查和前端构建通过。
