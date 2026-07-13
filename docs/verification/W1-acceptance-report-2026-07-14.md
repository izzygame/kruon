# Kruon W1 技术闭环验收报告

- 日期：2026-07-14
- 结论：技术门通过；实际用户访谈等待产品方招募 3～5 名参与者

## 验收证据

| 验收项 | 结果 | 证据 |
|---|---|---|
| Codex 真实只读探针 | 通过 | `codex-cli 0.144.2`；40 个事件；终态 `completed`；夹具未变化 |
| Claude 真实只读探针 | 通过 | Claude Code 2.1.205；28 个事件；终态 `completed`；费用上限 0.10 美元；夹具未变化 |
| 双协议统一与 SQLite 回放 | 通过 | 两种适配器均投影为 Run/Event；Rust 集成测试验证持久化、单终态和回放 |
| 重启恢复 | 通过 | 非终态 Run 追加恢复事件并进入 `uncertain`，不操作旧 PID |
| 取消、超时、强停、残留 | 通过 | 正常取消、TERM-resistant 进程组、超时、重复取消和并发竞态测试通过 |
| 路径越界拒绝 | 通过 | 合法子路径、`..`、绝对越界、符号链接逃逸和仓库探针路径守卫测试通过 |
| 数据最小化 | 通过 | 提示只存哈希；畸形行只存哈希；递归脱敏；子进程环境白名单；IPC 错误脱敏 |
| 既有回归 | 通过 | 原 177 项检查保持通过 |
| 新增 Rust 验证 | 通过 | 23 个核心测试 + 2 个探针守卫；fmt、all-targets check 通过 |
| ADR 与研究材料 | 通过 | ADR-001～005、访谈脚本、授权说明、录屏与归纳模板齐备 |
| 3～5 名真实用户访谈 | 待外部输入 | 产品方尚未提供参与者和录制材料 |

## 真实夹具

- Codex：`spikes/cli-adapters/fixtures/live/codex-read-only-live-redacted.json`，README SHA-256 `0ad6474e0d16060843a51e0cd13d3979055a2f485169eafc7e6906fd75091964`。
- Claude：`spikes/cli-adapters/fixtures/live/claude-read-only-live-redacted.json`，README SHA-256 `a1c03b70706062a3ba1967bd889f9ee72de69412eeef2d48eb4a6e55ddebce7c`。

每个适配器只执行一次真实模型探针；后续安全修订使用本地假 CLI 回归，没有再次调用模型。

## 不应对外宣称

W1 未实现网络出口隔离、系统级容器/Seatbelt 隔离、运行期间路径替换防护、`setsid` 逃逸阻断或 Rust 产品路径中的逐工具审批。进入可写执行或外部发布能力前必须重新开安全门。
