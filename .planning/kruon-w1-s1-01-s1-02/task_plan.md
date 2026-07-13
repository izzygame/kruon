# kruon W1 S1-01/S1-02 实施计划

## 目标

在当前机器缺少 Rust/Cargo 的约束下，推进工程骨架和领域状态机，不伪造 Tauri 构建证据；把主工程交给 OpenCode、领域支线交给 Hermes、机械复核交给 Mimo，并由 Codex 统一审查收敛。

## 分工与目录边界

- OpenCode：`apps/desktop/`、根 Node workspace 配置、`.github/workflows/`、`scripts/check-dev-env.mjs`、根 `README.md`/`.gitignore`。不得修改 `crates/` 与既有 `spikes/`。
- Hermes（Coding Plan / `deepseek-v4-flash-260425`）：`prototypes/domain-contract/`，独立审查并改进纯 TypeScript 领域对象、Run 状态迁移、事件重放和测试。不得修改根配置、`apps/`、`crates/` 与既有 `spikes/`。
- Mimo：仅承担依赖安装、工具链探测、离线复现等低风险机械任务；不做架构和领域判断。
- Codex：任务设计、目录越界检查、测试、架构审查；Rust 就绪后将已验证契约迁入正式 `crates/kruon-domain`。

## 阶段

| 阶段 | 状态 | 产出 |
|---|---|---|
| 1. 环境与边界冻结 | complete | 工具链证据、双执行器任务说明 |
| 2. OpenCode 工程骨架 | complete | React/Tauri 目录契约、Node workspace、CI、环境检查 |
| 3. Hermes 领域契约审查 | complete | Task/Run/Event、状态机、重放与测试 |
| 4. Codex 联合审查 | complete | 越界、构建、测试、契约一致性检查 |
| 5. 收敛与下一步 | complete | Rust 安装门、迁移清单、下一任务包 |

## 验收标准

- OpenCode 产物至少通过 `pnpm install --frozen-lockfile`（若 lock 存在）、typecheck、lint/test/build；Rust/Tauri 构建只能标为 blocked，不得伪装通过。
- 环境检查必须明确报告 Node、pnpm、Rust、Cargo 和平台依赖。
- Hermes 审查后的原型必须拒绝非法状态迁移，能从事件重建 Run 状态，并把 `completed` 与 `accepted` 分开。
- 双方不得修改对方目录或 PRD/规划/品牌材料。
- 所有未知终态保持 `uncertain`，不得猜测为完成。

## 错误记录

| 错误 | 尝试 | 处理 |
|---|---|---|
| 本机缺少 rustc/cargo/cargo-tauri | 1 | 不伪造构建；先完成不依赖 Rust 的骨架和契约原型，Rust 安装单独设门 |
| npm 官方源下载 TypeScript/esbuild 多次超时 | 3 | 切换 npmmirror 完成下载并生成锁文件；Mimo 用离线安装复核通过 |
| pnpm 11 拒绝未审批的 esbuild 构建脚本 | 2 | 在 `pnpm-workspace.yaml` 显式设置 `allowBuilds.esbuild: true` 后安装通过 |
| Claude CLI 路由模型持续思考但不调用工具 | 2 | 按用户要求停止使用 Claude，改由 Hermes DeepSeek V4 Flash 接手 |
