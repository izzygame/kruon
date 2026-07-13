# kruon W1 S1-01/S1-02 发现

- Node v22.22.3、npm 10.9.8、pnpm 11.8.0 可用。
- `rustc`、`cargo`、`cargo tauri` 均不可用，是正式 Tauri 构建的当前阻断点。
- OpenCode 1.17.18 可用；默认 `build` agent 已配置，但仍需通过实际小任务确认模型和写入通路。
- OpenCode 原生凭据列表为空、`models` 曾因数据库锁失败，但 `build` agent 的只读 smoke 成功返回 `OPENCODE_READY kruon`，说明现有 Coding Plan 通路可用。
- 既有 CLI adapter spike 保持只读，不在本轮修改。
- Hermes 实时 one-shot 探针返回 `HERMES_READY`，usage 记录确认模型为 `deepseek-v4-flash-260425`。
- Mimo 已安装并可执行；本轮机械复核确认 `pnpm install --offline` 成功，锁文件可复现。
- pnpm 11 的依赖构建许可位于 `pnpm-workspace.yaml` 的 `allowBuilds`，不能再放在根 `package.json` 的 `pnpm` 字段。
