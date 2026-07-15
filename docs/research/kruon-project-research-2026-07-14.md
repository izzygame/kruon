# kruon 项目调研报告

> 调研人：产品经理 许清楚（Xu） ｜ 日期：2026-07-14 ｜ 方法：一手代码 + 文档复核（非网络实时核验）

## 0. 一句话定位
kruon 是一个**本地优先**的桌面工作空间，让用户在可信环境里启动、观察、干预并验收已有的 AI 编码 CLI（Codex / Claude Code）；3D 只是把同一份事件流变成"有生命感的世界"，不是核心。

## 1. 项目概览
| 维度 | 内容 |
|---|---|
| 产品定位 | 面向个人创作者与小型团队的"AI 工作指挥空间"，本地优先、BYO 工具/订阅 |
| 目标用户 | 个人创作者、小型团队；重度并行使用多款本地 AI CLI 的开发者 |
| 核心价值 | 跨厂商本地统一控制平面：连接→任务→权限→取消→产物→人工验收的**真实闭环**（不是启动器） |
| 首发平台 | macOS（aarch64）优先；Windows Beta 之后（需独立 Job Object 实现） |
| 技术栈 | Tauri 2（壳）+ React/TS（2D 默认）+ React Three Fiber（3D 懒加载可卸载）+ Rust 本地核心 + SQLite（事件追加）+ macOS Keychain（凭据，UI 尚未接入） |
| 品牌动作 | "switch your crew on / 一键开启你的 AI 小队"；关键词 friendly/intelligent/connected/alive |
| 当前阶段 | W1（M0 技术门）已通过；前端与 M1+ 尚未开始 |

## 2. MVP 范围
### P0（Alpha 必交付）
| 模块 | 说明 |
|---|---|
| 工具连接 | Codex + Claude Code 深适配；发现 / 版本 / 认证 / 能力检查 |
| 统一任务/运行模型 | 同一 Run/Event 模型；Idle→Completed 等 8 状态 |
| 权限中心 | 参数绑定审批、有效期、批准/拒绝/缩小范围；审批模式诚实展示 |
| 2D 控制台 | 默认主界面：任务板、审批、交付、连接、设置 |
| 最小 3D 状态映射 | 固定低多边形办公室；6–8 真实状态；可完全关闭、不阻塞控制 |

### P1（可独立验证增强）
- 经来源/作用域治理的共享记忆（项目简报/决策/约束，提案式写入与跨工具复用）
- 双 Agent 脑暴（独立产出→反方评审→人类决策→结构化结论）

### 明确非目标（不进 MVP）
自动模型路由、网页端自动化、额度代理、跨设备同步、远端 Agent、插件市场、广告、自由建造、角色养成、实时多人、完整群体模拟。

## 3. 技术架构与模块边界
### 3.1 同源事件架构（核心约束）
- 单一规范化 `Event` 流；2D 控制台、3D 世界视图、通知、诊断**只消费这一来源**（2D/3D 同源）。
- 3D 不持有独立状态、不执行、不审批；关闭/卸载 3D 后所有控制能力完整可用（代码体现：EventStore 是唯一真相源，`replay_run` 可重建终态）。
- Tauri capability 需限制 3D/展示窗口不拿进程、写文件、凭据能力（ADR 已规划，实现待 M3）。

### 3.2 模块职责与实现状态
| 模块 | 职责 | 禁止承担 | 当前状态 |
|---|---|---|---|
| Domain Core | Workspace/Policy/Task/Run/Event/Approval/Artifact 状态机 | CLI 文本解析、UI 状态 | ✅ domain.rs 已实现（Python 原型 state-machine.ts 已先行验证） |
| Policy Engine（路径） | 文件越界/符号链接/绝对路径守卫 | 直接执行动作 | ✅ path_policy.rs 已实现+测试 |
| Process Supervisor | 启动/信号/进程组/超时/退出/恢复/残留 | 判断业务完成 | ✅ process_supervisor.rs（SIGTERM→10s→SIGKILL，残留检测，测试齐备） |
| Adapter Host | 版本探测、IO、能力降级 | 绕过 Domain/Policy 写库 | ⚠️ Rust 端 `launch_plan` 固定写死；spikes 的 capability manifest 未接入产品路径 |
| Event Store | 追加事件、快照、迁移、崩溃恢复、对账 | 存上游凭据 | ✅ event_store.rs（recover_interrupted_runs、replay_run） |
| Artifact Service | 文件/diff/测试/报告识别与验收归档 | 自动判定接受 | ❌ 未开始 |
| Desktop UI（2D） | 任务/审批/交付/连接/设置 | 直接访问进程/凭据 | ⚠️ 仅 App.tsx 占位（"shell ready"），无业务组件 |
| World View（3D） | 规范事件→固定空间状态 | 执行/审批/维护独立状态 | ❌ 未开始（R3F 未安装） |

### 3.3 已实现 Rust 核心证据
- 文件：`src-tauri/src/core/{domain,error,event_store,path_policy,process_supervisor,adapter_host,runtime,mod}.rs`、`lib.rs`、`main.rs`、`bin/w1_probe.rs`
- Tauri 命令：`start_run / cancel_run / get_run / list_events / replay_run`
- 真实只读探针：**Codex `0.144.2`**（40 事件、终态 completed、夹具未变），**Claude `2.1.205`**（28 事件、completed、预算 $0.10）
- W1 验收报告记录：23 个 Rust 核心测试 + 2 个探针守卫；原有 177 项检查保持通过（具体构成待确认）；fmt / all-targets 通过
- 安全审查（Hermes + Codex，2026-07-14）：env_clear 白名单、IPC 错误脱敏、取消竞态、路径守卫等有效问题已修复

## 4. 当前真实进度评估（对照里程碑）
### 总体判断：W1 后端闭环"技术门"已通过；前端（M1–M4）尚未开始。
| 里程碑 | 范围 | 状态 | 关键证据 |
|---|---|---|---|
| **M0 (W1-W2)** 阻断风险验证 | 骨架/契约/spike/威胁模型/SQLite/ADR | ✅ 基本完成（~95%） | 5 个 ADR、Rust 核心、Codex+Claude 真实探针、取消/路径/回放测试全过；**唯一缺口：用户访谈（DEV-009）未开始**，W1 验收报告标注"待外部输入" |
| **M1 (W3-W4)** 控制骨架 | 2D 控制台、连接、Task/Run、队列 | ❌ 未开始 | `src/` 仅 App.tsx 占位；无连接/任务/审批组件；Rust 后端已具备 start/cancel 但无 UI 消费 |
| **M2 (W5-W6)** 任务控制闭环 | 审批指纹/取消恢复/产物/验收 | ❌ 未开始 | Artifact Service、Approval 模型、验收 UI 均无代码 |
| **M3 (W7-W8)** 最小世界视图 | 3D 状态映射 | ❌ 未开始 | 前端依赖无 three.js/R3F；无 3D 代码 |
| **M4 (W9-W10)** 邀请制 Alpha | 兼容/安全/打包/用户 | ❌ 未开始 | 无签名/公证/安装/更新逻辑 |

### 进度与计划的差距
- **正向超预期**：W1 把最难的"本地 CLI 控制 + 安全 + 事件溯源"做成了可运行后端，且带真实只读探针与严格 fail-closed 语义（如 `uncertain` 不伪装完成）。工程质量高（强类型、幂等事件、脱敏、残留检测、测试齐备）。
- **关键落差**：计划假设 W1 末有"双 CLI 只读任务 + SQLite 回放 + 取消实验 + 越界拒绝"演示——**已达成（w1_probe 即该演示）**。但计划 M1 起进入 2D 控制台，而当前**前端是空白**。git 提交全为 2026-07-14 单次密集推进，符合"1 名工程师"画像；若确为单人，10 周整体目标需下调为计划明示的 16–18 周。
- **spike 与产品路径断层**：`spikes/cli-adapters` 的 capability manifest / event parser / 指纹逻辑是 **Python 原型**，尚未迁移进 Rust 产品路径。Rust 的 `AdapterHost.launch_plan` 目前为**固定写死**的 Codex/Claude 命令模板（ADR-003）。即"适配器能力分级 / 版本探测 / 审批模式差异"在生产代码中尚未兑现，只在文档与 Python 原型里。

## 5. 竞争格局与差异化判断
> 注：findings.md 明确标注竞品的星数/价格/"市场空白"判断可能过时，**需外部一手资料复核**（本次以 findings 证据分层为准，未做实时网络核验）。

### 已识别竞品（来自 findings）
| 竞品 | 覆盖 | 对 kruon 的含义 |
|---|---|---|
| **Paperclip** | BYO Agent、任务票据、成本预算、审批、审计、多 Agent 控制平面；"自主 AI 公司"隐喻 | 不能声称"控制平面无人占据"；聚焦**本地 CLI、项目工作目录、2D+真实状态世界** |
| **CC Switch** | 多工具配置、本地路由、failover、用量、本地密钥 | 不应重造供应商路由；价值放在**任务/权限/产物/验收闭环** |
| **CLAW3D / openclaw-office / agent-office / AI Agent Session Center** | 3D/2.5D 办公室、状态动画、协作线、气泡 | "3D 办公室"已是**拥挤形态**，不能作为唯一差异化 |
| **OpenAI Codex App** | 多 Agent 并行监督、长任务界面 | "多任务监督"已成平台基线；须强调**跨厂商+本地统一** |
| **协议层 ACP / A2A / MCP** | ACP 贴近本地深适配；A2A 适合远端；MCP≠任务编排 | 须保留适配器能力分级，不被协议替代本地控制 |

### 差异化判断：部分成立，需收敛叙事
- ✅ **真实成立**：`本地优先 + 跨异构 CLI 的真实闭环（审批/取消/产物/人工验收） + 项目目录即真相源 + 2D 优先、3D 仅作"有生命感"状态投影 + 诚实的审批能力不对称表达`。这是现有竞品（尤其 3D 办公室类）未扎实覆盖的缝隙。
- ⚠️ **不成立/危险**："3D AI 办公室"本身**不是差异化**（已拥挤）。若以 3D 为卖点，会直接撞上 CLAW3D/openclaw-office。
- ⚠️ **被蚕食风险**：控制平面价值正被 Paperclip（审批/审计/多 Agent）与 CC Switch（路由/密钥）侵蚀。kruon 必须靠"**本地 CLI 真相 + 验收模型 + 跨工具诚实审批**"取胜，而非功能广度。
- 📌 团队在 findings 与 PRD 中已正确把 3D 降级为"可关闭的真实状态视图"、把叙事从"招聘虚拟员工"转为"已有工具组队"，方向正确。

## 6. 关键风险与未决决策（按严重度排序）
### 🔴 R1. 前端/闭环尚未落地，"漂亮启动器"风险最高
现状：全部价值承诺（任务创建→审批→取消→产物→验收）**零 UI**；Rust 后端再强，用户看不到闭环。findings 已预警"适配器只能启动不能返回状态/权限/产物/取消 → 退化为漂亮启动器"。
建议：M1/M2 必须按"每双周一场真实 CLI 演示、不用 mock"铁律推进；W4 Go/Pivot 门严格判断"是否减少工具切换"；**不要被 3D 抢预算**。

### 🔴 R2. Codex 逐动作审批在 MVP 不可得，产品诚实性风险
现状：`codex exec` 无 `--ask-for-approval`（仅 root 交互式有）；W1 决策为 `sandbox_policy_only`（read-only/workspace-write）。Claude 走 `manual` 权限模式（per_action），但**真实双向审批回传未验证**（spike README 标 inferred）。M2 的 DEV-201 要求 per_action 高风险动作先产生绑定参数的审批请求——对 Codex 不成立。
决策待定：① 仅对 Claude 呈现逐动作审批，Codex 明确标 `sandbox_policy_only` 且不伪装；② 或为 Codex 用"前置策略冻结 + 事件后检测"作审批替代并写明差异；③ 监控 `app-server` 从 experimental 毕业（W2+ 重评）。**无论如何，不得把 sandbox_policy_only 表成 per_action。**

### 🟠 R3. 安全边界未达 OS 级隔离，外发前必须重开安全门
现状（ADR-004 / 安全审查明确"不宣称"）：无网络出口隔离、无容器/Seatbelt、有 TOCTOU 路径替换窗口、`setsid` 逃逸不可挡、PID 复用影响残留判定、`uncertain` 输出线程不可 join。
关联：W1 仅做"受控只读探针"。一旦进入**可写执行 / 自动审批 / 外部发布**，必须依 ADR-004 复审条件优先加系统级沙箱与出口策略。

### 🟠 R4. 用户研究缺口，MVP 假设未经验证
现状：DEV-009（3–5 名访谈）**未开始**，W1 验收标注"待外部输入"。W4 门"统一任务/事件/策略是否减少工具切换，还是增加配置负担"**无法在没有用户的情况下回答**；脑暴材料大量方向标"待验证"。
建议：产品方立即招募；W2 起每周 3–5 访谈，用 `docs/research/w1-user-interviews/` 模板录屏与归纳。

### 🟠 R5. 资源/排期风险（单人团队与 10 周目标）
现状：git 全部提交于 2026-07-14 单次密集推进，符合"1 名工程师"画像；计划本身写明"若仅 1 人则 16–18 周，不得削减权限/取消/验收门槛换 10 周表面完成"。
建议：对外承诺采用 16–18 周口径或明确"1 名工程师"约束；任何人员变动即重排期。

### 🟡 R6. 单 SQLite 连接（Mutex）对并行 Run 的吞吐瓶颈
现状：W1 用 `Mutex<Connection>` 保守串行；ADR-002 已标为吞吐权衡。M1 退出标准"两个隔离 Run 并行、状态不串线"——串行写入在双 Run + 长输出时可能成延迟点，须尽早压测。

### 🟡 R7. spike→产品路径断层与 Windows 兼容
现状：capability manifest / event parser / 指纹在 Python 原型，未入 Rust；仅 macOS/aarch64 验证；Windows 需独立 Job Object（ADR-003 复审条件）。

## 7. 给团队/创始人的下一步建议（5 条）
1. **锁死"后端已验证、前端为王"的优先级**：W2–W4 全部火力做 2D 控制台（连接→Task→Run 板→审批/取消→产物/验收），把"真实 CLI 演示"作为唯一验收标准；3D 推迟到 M3 且可关闭。
2. **立即补齐用户研究**（DEV-009）：本周启动 3–5 名访谈，否则 W4 Pivot 门无依据。
3. **在产品层固化审批能力不对称**：明确 Codex=`sandbox_policy_only`、Claude=`per_action(待验证回传)`，UI 与测试必须诚实区分，杜绝"伪逐动作审批"。
4. **设安全发布闸门**：把"可写执行/自动审批/外发"列为硬 gate，进入前必须补 OS 级沙箱/出口策略（ADR-004 复审）。
5. **对外排期用 16–18 周口径**（按 1 名工程师现实），保护 P0 门槛不被压缩；并把 Python spike 的 capability/event 逻辑迁移进 Rust 作为 M1 前置。

## 8. 待确认问题清单
- [ ] 当前实际工程人力配置？是否确为 1 名工程师？（影响 10 周 vs 16–18 周）
- [ ] 3–5 名目标用户访谈的招募责任人与时间表？
- [ ] M2 对 Codex 的审批呈现方案：sandbox_policy_only 直说 / 前置策略冻结替代 / 等 app-server 毕业？（W2 前定）
- [ ] 单 SQLite 串行连接在双 Run 长输出下的压测结果？是否需提前引入写者队列？
- [ ] Python spike（capability manifest / event parser / 指纹）迁移进 Rust 的排期与负责人？
- [ ] Windows Beta 的 Job Object 与 macOS 签名/公证/自动更新的责任归属？
- [ ] 外部竞品（Paperclip/CC Switch/CLAW3D 等）的**实时**功能与定价是否需重新核验以更新 findings？
- [ ] W1 验收报告提及的"原 177 项检查"具体构成（是否含前端/契约测试）？

---
### 附：一手证据索引（本调研读取）
- 工程：`apps/desktop/src-tauri/src/core/*`、`lib.rs`、`bin/w1_probe.rs`、`src/App.tsx`、`Cargo.toml`、`package.json`、`tauri.conf.json`
- 验收/安全：`docs/verification/W1-acceptance-report-2026-07-14.md`、`docs/security/w1-runtime-security-review-2026-07-14.md`、`docs/adr/ADR-001~005`
- spike：`spikes/cli-adapters/README.md`、`spikes/codex-service-protocol/{README,decision}.md`
- 原型：`prototypes/domain-contract/README.md`
- 规划/调研：`kruon_MVP开发计划_2026-07-11.md`、`findings.md`、`task_plan.md`、`progress.md`、`README.md`、`brand/kruon-brand-brief-v0.1.md`
- 未实时核验：竞品外部一手资料、PRD/路线图 docx 正文（仅依 findings/开发计划复核）、`参考.mp4` 视频（依 findings 文字结论）
