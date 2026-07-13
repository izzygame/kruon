# macOS 进程隔离方案决策输入 v1

> **文档标识**: S1-06-SI-v1  
> **依据**: `workspace-policy-threat-model-v1.md` T-01/T-02/T-04/T-09/T-05  
> **代码基**: `spikes/cli-adapters/` (probe.py, event_parser.py, capability_manifest.py)  
> **证据等级**: `verified` = 代码/测试已覆盖; `inferred` = 架构设计但未运行验证; `unverified` = 尚未实现或测试; `spike_needed` = 需实机验证  
> **日期**: 2026-07-14  
> **作者**: S1-06 安全审查支线  
> **约束**: 本文件只做实现决策输入，不把 sandbox-exec/Seatbelt 或 pf 当成已选定方案

---

## 目录

1. [评估范围与输入约束](#1-评估范围与输入约束)
2. [领域一：进程组创建与 SIGTERM/SIGKILL 取消](#2-领域一进程组创建与-sigtermsigkill-取消)
3. [领域二：子进程残留检测](#3-领域二子进程残留检测)
4. [领域三：文件系统越界与符号链接 TOCTOU](#4-领域三文件系统越界与符号链接-toctou)
5. [领域四：网络出口限制](#5-领域四网络出口限制)
6. [领域五：环境变量清理](#6-领域五环境变量清理)
7. [候选方案矩阵](#7-候选方案矩阵)
8. [P0 测试清单](#8-p0-测试清单)
9. [推荐实现顺序](#9-推荐实现顺序)
10. [失败回退策略](#10-失败回退策略)
11. [明确非目标](#11-明确非目标)

---

## 1. 评估范围与输入约束

### 1.1 架构上下文

kruon 的 Process Supervisor（计划中的 Rust 模块）负责管理 CLI 子进程生命周期。当前 spike 阶段（`probe.py`）使用 Python `subprocess.Popen` 直接管理子进程，生产实现将迁移到 Rust 的 `std::process::Command` 或 `nix` crate。

```
kruon Process Supervisor (Rust, future)
  ├── spawn CLI subprocess (Codex/Claude)
  ├── manage process group (setsid / pgid)
  ├── SIGTERM → deadline → SIGKILL
  ├── orphan / zombie detection
  ├── filesystem boundary enforcement
  ├── network egress restriction
  └── env sanitization
```

### 1.2 攻击者模型（本文件关注）

| 攻击者 | 能力 | 本文件防御目标 |
|--------|------|---------------|
| A1: Prompt 注入 | 操纵 CLI 执行非预期动作 | 限制 CLI 子进程的能力边界，使其即使被操纵也无法越界 |
| A2: 恶意文件名 | 控制文件名/路径/符号链接 | 阻止 TOCTOU 和符号链接越界写入 |
| A5: 网络中间人 | 拦截/篡改通信 | 不是本文件目标（依赖 TLS） |

### 1.3 当前代码已验证的防御基线

来自 `spikes/cli-adapters/` 的 verified 项（本文件不再重复评估）：

- ✅ `_path_in_workspace()`: 使用 `os.path.realpath()` 规范化路径后检查前缀匹配
- ✅ `_check_safe_args()`: 拒绝 DANGEROUS_FLAGS
- ✅ `determine_approval_mode()`: 拒绝 `bypassPermissions`
- ✅ `fingerprint_params()`: 参数指纹生成
- ✅ `redact()`: 6 种 token 格式 + 环境变量模式脱敏
- ✅ `resolve_cancel_state()`: 取消状态分类逻辑
- ✅ `classify_terminal()`: 从不将冲突终态强制为 completed

---

## 2. 领域一：进程组创建与 SIGTERM/SIGKILL 取消

### 2.1 当前代码状态

`probe.py:execute_model_call()` (lines 311-380):

```python
proc = subprocess.Popen(plan.argv, cwd=plan.workspace, ...)
# ... read loop ...
if proc.poll() is None:
    proc.terminate()       # SIGTERM
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()         # SIGKILL
```

**关键发现**: 只终止直接子进程。CLI 可能已通过 `subprocess.Popen` 或 `os.system` 派生孙子进程，这些进程在父进程被 SIGTERM 后成为孤儿，继续运行。

### 2.2 macOS 进程组机制评估

| 机制 | macOS 支持 | 说明 |
|------|-----------|------|
| `os.setpgid()` / `setpgid(2)` | ✅ 完全支持 | 将子进程放入新进程组；SIGTERM/SIGKILL 发送到整个组 |
| `setsid(2)` | ✅ 完全支持 | 创建新会话，子进程成为会话 leader；更彻底的隔离 |
| `kill(-pgid, SIGTERM)` | ✅ 完全支持 | 向整个进程组发送信号 |
| macOS `proc_pidlistuptrs(2)` | ✅ 支持 | 遍历进程树，检测残留 |
| macOS `proc_listchildpids(2)` | ✅ 支持 | 列出指定 PID 的所有子进程 |

**代码证据**: 当前 spike 未使用任何进程组机制（`verified` — 代码中无 `setpgid`/`setsid` 调用）。

### 2.3 决策点

| 选项 | 复杂度 | 可靠性 | 备注 |
|------|--------|--------|------|
| A: `os.setpgid(0, 0)` 在子进程 fork 后 | 低 | 高 | Rust 中 `Command::new().process_group(0)` 或 `unsafe` 的 `libc::setpgid` |
| B: `setsid()` 在子进程 fork 后 | 中 | 更高 | 创建新会话，隔离更彻底；但可能影响 CLI 的终端信号处理 |
| C: 不设进程组，仅 kill 直接子进程 | 低 | 低 | 当前状态，残留风险高 |

**推荐**: W1 采用选项 A（`setpgid`），因为：
- 与 CLI 的终端信号处理兼容性最好
- Rust `std::process::Command` 的 `process_group(0)` 直接支持
- 可通过 `kill(-pgid, SIGTERM)` 可靠地终止整个进程树

**需要 spike 验证**:
- `spike_needed` — Claude Code 和 Codex CLI 在收到 SIGTERM 到整个进程组时的行为（是否正常退出 vs 需要 SIGKILL）
- `spike_needed` — CLI 自身是否创建进程组（如 Claude 的 `claude` 进程是否已 `setsid`），影响 pgid 策略

### 2.4 取消时间线（威胁模型 G-04/G-05 要求）

```
t=0    用户取消 → Process Supervisor 发送 SIGTERM 到整个进程组
t=0-5s 等待 CLI 响应（stdout 关闭 / 终端事件到达）
t=5s   若未响应 → 发送 SIGKILL 到整个进程组
t=5-10s 等待进程组退出
t=10s  若仍未退出 → 标记 forced_stop_required，日志告警
```

**当前状态**: `inferred` — `resolve_cancel_state()` 函数逻辑已通过单元测试，但真实超时循环和进程组 kill 未实现。

---

## 3. 领域二：子进程残留检测

### 3.1 当前代码状态

无残留检测实现。`probe.py` 在读取循环结束后调用 `proc.poll()` 检查直接子进程是否存活，但不扫描系统进程表。

### 3.2 macOS 残留检测方案评估

| 方案 | 依赖 | 可靠性 | 说明 |
|------|------|--------|------|
| A: `kill(pid, 0)` 轮询 | 无额外依赖 | 中 | 只能检测指定 PID 是否存活；无法发现未知子进程 |
| B: `proc_listchildpids(2)` | macOS 系统库 | 高 | 遍历进程树，发现所有子/孙进程 |
| C: `sysctl(KERN_PROC)` | macOS 系统库 | 高 | 遍历全部进程表，按 PID 过滤 |
| D: `ps` 命令解析 | 无 | 低 | 不可靠，输出格式不稳定 |
| E: 进程组残留检测 | 依赖 setpgid 实现 | 高 | 检查进程组中是否还有存活进程 |

**推荐**: W1 采用方案 E（进程组残留检测），因为：
- 与领域一的 `setpgid` 天然配合
- 只需检查进程组 ID 是否还有存活进程
- 不依赖额外的系统调用知识

**Rust 实现路径**:
```rust
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;

fn kill_process_group(pgid: Pid) -> Result<()> {
    killpg(pgid, Signal::SIGTERM)?;
    // wait with timeout...
}

fn has_survivors(pgid: Pid) -> bool {
    // Use proc_listchildpids or killpg(pid, 0) check
}
```

### 3.3 需要 spike 验证

- `spike_needed` — Rust `nix` crate 的 `killpg` 在 macOS 上的行为（确认进程组所有成员都收到信号）
- `spike_needed` — Codex/Claude 在 SIGTERM 后子进程的典型退出延迟分布（为超时策略提供数据）
- `spike_needed` — 如果 CLI 自身调用了 `setsid()`，进程组策略需要调整

---

## 4. 领域三：文件系统越界与符号链接 TOCTOU

### 4.1 当前代码状态

**路径越界检测** (`event_parser.py:_path_in_workspace()`, lines 159-174):
- `verified` — 使用 `os.path.realpath()` 规范化路径后检查前缀匹配
- 但这是**事后检测**（artifact 事件到达后才标记），不阻止写入

**符号链接 TOCTOU**:
- `unverified` — 完全无运行时保护
- `_path_in_workspace()` 能检测已存在的符号链接指向越界
- 但在检查后、写入前，链接目标可能被替换

### 4.2 macOS 文件系统隔离方案评估

| 方案 | macOS 支持 | 复杂度 | 可靠性 | 说明 |
|------|-----------|--------|--------|------|
| A: 写入前 `realpath` 重新检查 | ✅ | 低 | 低 | 不能消除 TOCTOU 窗口，只能缩小 |
| B: `open(2)` 的 `O_NOFOLLOW` | ✅ | 中 | 中 | 拒绝打开符号链接，但 CLI 内部操作不受控制 |
| C: macOS Sandbox (sandbox-exec/Seatbelt) | ✅ | 高 | 高 | 内核级强制访问控制；但需要 profile 管理 |
| D: 工作区绑定挂载 (bindfs/nullfs) | ✅ | 中 | 中 | 创建只读/受限的挂载点；需要 root 或 `sandbox-exec` |
| E: 文件描述符传递 + 限制 syscall | ❌ | 极高 | 高 | 需要 ptrace/DTrace，超出 MVP 范围 |
| F: 运行时路径审计（fanotify 等效） | ❌ macOS 无 fanotify | — | — | macOS 无等效机制；`kqueue` 不能拦截文件操作 |
| G: 策略级防御（写入前冻结路径） | ✅ | 低 | 中 | Policy Engine 在 artifact 事件到达时检测越界并终止后续动作 |

**关键约束**: macOS 没有 Linux `fanotify` 或 `inotify` 的等效拦截机制。`kqueue` 只能通知不能阻止。因此运行时文件系统拦截在 macOS 上**必须依赖** Sandbox / Seatbelt 或用户态文件系统。

**O_NOFOLLOW 关键约束**: `O_NOFOLLOW` 仅对调用 `open()` 的进程自身有效。kruon 的 Adapter Host 无法对上游 Codex/Claude 自己发起的 `open()` 系统调用附加 `O_NOFOLLOW`，除非满足以下任一条件：
1. 所有写入操作均经过 kruon 的文件代理/受控协议（当前架构不满足）
2. 依赖系统级沙箱（macOS Sandbox/Seatbelt）或用户态文件系统进行拦截

因此 `O_NOFOLLOW` 不能作为当前 Adapter Host 层可直接实施的 W1 防线。

### 4.3 分层防御策略（推荐）

```
Layer 1: Policy Engine 事后检测（W1 可实现）
  - artifact 事件到达时重新解析路径
  - 与 launch plan 时记录的路径比较
  - 越界 → 立即终止后续动作，标记 failed/unknown
  - 局限：事后检测，无法阻止已发生的写入；仅能终止后续动作并标记失败

Layer 2: macOS Sandbox / Seatbelt（W2+ 评估）
  - 内核级强制访问控制
  - 限制 CLI 子进程的文件系统可见性
  - 需要：评估当前 macOS 版本（26.5.1）的 sandbox-exec 兼容性
  - 需要：编写 Seatbelt profile 限制写入到 workspace 目录
  - 风险：sandbox-exec 在较新 macOS 版本上的行为变化

Layer 3（理论候选，依赖系统级沙箱）: O_NOFOLLOW 在受控写入路径上
  - 仅当所有写入经 kruon 文件代理/受控协议时可行
  - 或依赖 macOS Sandbox 强制实施
  - W1 不可行，归入 W2+ 与 Sandbox 一并评估
```

### 4.4 符号链接 TOCTOU 具体攻击场景

```
时间线:
t0: CLI 检查文件 /workspace/link 是否存在 → 存在（指向 /workspace/ok.txt）
t1: 攻击者（通过 prompt 注入）删除 link 并创建 link -> /etc/passwd
t2: CLI 写入 /workspace/link → 实际写入 /etc/passwd
```

**防御**: 在 t2 之前，Policy Engine 无法感知链接替换。唯一可靠的防御是：
1. macOS Sandbox 限制写入范围（Layer 2）
2. 或者所有写入经 kruon 文件代理后使用 `O_NOFOLLOW` + `open()` 原子检查（Layer 3，依赖系统级沙箱）

**当前状态**: `unverified` — 无运行时保护。威胁模型已正确识别为高剩余风险。W1 仅能通过 Policy Engine 事后检测缓解（终止后续动作、标记失败），无法阻止已发生的写入。

### 4.5 需要 spike 验证

- `spike_needed` — macOS 26.5.1 上 `sandbox-exec` 是否可用、profile 语法是否兼容
- `spike_needed` — `sandbox-exec` 对 Codex/Claude CLI 的影响（是否破坏正常功能）
- `spike_needed` — Seatbelt profile 能否精确限制到 workspace 目录（允许读写 workspace，拒绝其他）
- `spike_needed` — 绑定挂载方案（`mount_nullfs` 或 `bindfs`）是否需要 root 权限

---

## 5. 领域四：网络出口限制

### 5.1 当前代码状态

`unverified` — 完全无网络出口控制。CLI 子进程有完整的网络访问权限。

**严重性**: 威胁模型标记为「极高剩余风险」。kruon 无法区分合法 LLM API 调用和恶意外传。

### 5.2 macOS 网络出口限制方案评估

| 方案 | macOS 支持 | 复杂度 | 可靠性 | 说明 |
|------|-----------|--------|--------|------|
| A: 应用层代理配置 | ✅ | 低 | 低 | 设置 `HTTP_PROXY`/`HTTPS_PROXY` 环境变量；CLI 可能忽略 |
| B: macOS `socketfilterfw` (ALF) | ✅ | 中 | 中 | 系统级防火墙，按进程/pid 控制；需要授权 |
| C: `pfctl` (packet filter) | ✅ | 高 | 高 | BSD packet filter，按 uid/gid/jail 过滤；需要 root |
| D: macOS Sandbox (network-outbound deny) | ✅ | 中 | 高 | Seatbelt profile 中 `(deny network-outbound)` 但需允许特定域名 |
| E: 用户态 DNS/连接监控 | ✅ | 中 | 低 | 监控但不阻止；只能检测不能防御 |
| F: 网络命名空间（无 Linux 等效） | ❌ | — | — | macOS 无网络命名空间 |
| G: SOCKS5 代理 + 白名单 | ✅ | 中 | 中 | 通过代理转发所有流量，代理端白名单 |

### 5.3 关键约束

1. **CLI 需要访问上游 LLM API**: Codex 需要访问 `api.openai.com`（或自定义端点），Claude 需要访问 `api.anthropic.com`。不能简单阻止所有网络访问。
2. **域名白名单需要维护**: 上游 API 域名可能变化（如 Claude 的区域端点）。
3. **macOS 无网络命名空间**: 无法像 Linux 那样将进程放入隔离的网络栈。
4. **`pfctl` 需要 root**: 在桌面应用中请求 root 权限的 UX 负担高。

### 5.4 推荐策略（W1 候选暂缓建议）

```
W1 候选暂缓建议:
  - 不实现网络出口限制
  - 依赖用户的工作区信任 + 审批机制
  - 在 UI 中明确提示网络风险
  - 记录 CLI 子进程的网络连接（DNS 查询日志）用于审计

W2+ 评估方案:
  - macOS Sandbox (network-outbound) + 白名单域名
  - 或 socketfilterfw 按进程控制
  - 或 SOCKS5 代理 + 代理端白名单
```

**当前状态**: 网络出口限制列为 W1 候选暂缓建议，**非最终 scope 决策**。在产品负责人显式接受以下剩余风险前，不得视为已关闭：

| 剩余风险 | 说明 | 接受条件 |
|---------|------|---------|
| R1: 恶意 CLI 外传数据 | 被注入的 CLI 可将工作区数据外传到任意网络端点 | 需产品负责人显式签字接受 |
| R2: 无法区分合法与恶意流量 | kruon 无法区分 LLM API 调用和数据窃取 | 依赖审批机制缓解，但审批不覆盖网络层 |
| R3: 无审计时无法追溯 | 无网络审计日志时无法调查外传事件 | W1 可选实现 DNS 查询日志 |

**建议**: W1 冲刺期间保持此暂缓状态，在 W1 结束演示中向产品负责人展示上述剩余风险清单，获取显式接受或要求 W2 优先实现。

---

## 6. 领域五：环境变量清理

### 6.1 当前代码状态

`capability_manifest.py:LaunchPlan` 包含 `env_redacted: Dict[str, str]` 字段，但始终为空字典。

`probe.py:execute_model_call()` 使用 `subprocess.Popen(..., env=None)`，即**继承宿主环境**。

**风险**: 宿主环境中的敏感变量（`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `AWS_ACCESS_KEY_ID`, `GITHUB_TOKEN`, `SSH_AUTH_SOCK`, `PATH` 中的恶意条目等）被透传给 CLI 子进程。

### 6.2 环境变量分类

| 类别 | 示例 | 处理策略 |
|------|------|---------|
| 上游 CLI Token | `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` | ❌ 不传递（CLI 自行从 Keychain 读取） |
| 系统路径 | `PATH`, `HOME`, `USER` | ✅ 传递（清理后） |
| 代理设置 | `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | ✅ 传递（用户可能依赖） |
| 开发环境 | `NODE_ENV`, `PYTHONPATH`, `GOPATH` | ⚠️ 按需传递 |
| 敏感凭据 | `AWS_ACCESS_KEY_ID`, `GITHUB_TOKEN`, `DB_PASSWORD` | ❌ 不传递 |
| SSH 代理 | `SSH_AUTH_SOCK`, `SSH_AGENT_PID` | ❌ 不传递（防止 CLI 使用 SSH 密钥） |
| 编辑器/工具 | `EDITOR`, `VISUAL`, `PAGER` | ✅ 传递 |

### 6.3 推荐的环境清理策略

```python
# 允许列表（允许传递给 CLI 子进程的环境变量）
ALLOWED_ENV_KEYS = {
    "PATH", "HOME", "USER", "LOGNAME", "SHELL",
    "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE",
    "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
    "TERM", "COLORTERM",
}

# 拒绝模式（明确禁止传递的变量名模式）
DENIED_PATTERNS = (
    "API_KEY", "APIKEY", "SECRET", "TOKEN", "PASSWORD",
    "CREDENTIAL", "ACCESS_KEY", "AUTH_TOKEN",
    "SSH_AUTH", "SSH_AGENT",
    "AWS_", "AZURE_", "GCP_", "GOOGLE_APPLICATION_CREDENTIALS",
)
```

### 6.4 需要 spike 验证

- `spike_needed` — Codex CLI 和 Claude Code CLI 是否依赖环境变量中的 API key（还是只从 Keychain 读取）
- `spike_needed` — 移除 `SSH_AUTH_SOCK` 是否影响 CLI 的 git 操作（CLI 可能需要通过 SSH 拉取私有仓库）
- `spike_needed` — 如果 CLI 需要 git SSH 访问，是否需要保留 `SSH_AUTH_SOCK` 或通过其他机制提供

---

## 7. 候选方案矩阵

### 7.1 W1 可执行候选

| 领域 | 候选方案 | 证据等级 | 实现复杂度 | 防护效果 | W1 可行 |
|------|---------|---------|-----------|---------|--------|
| 进程组 | A: `setpgid(0,0)` + `kill(-pgid, SIGTERM/SIGKILL)` | `spike_needed` — 机制已知，但 CLI 兼容性需 spike 验证 | 低 | 高 | ✅ |
| 残留检测 | A: 进程组存活检查 + `proc_listchildpids` | `spike_needed` — 需 spike 验证 macOS API 及 CLI 进程组行为 | 中 | 中 | ✅ |
| 文件系统 L1 | Policy Engine 事后检测（artifact 越界 → 终止后续动作，标记 failed/unknown） | `inferred` — 架构设计存在，未实现 | 低 | 低（仅检测，不阻止写入） | ✅ |
| 文件系统 L2 | macOS Sandbox / Seatbelt | `spike_needed` — 需验证兼容性 | 高 | 高 | ❌ W2+ |
| 文件系统 L3 | O_NOFOLLOW 在受控写入路径上 | `spike_needed` — 依赖系统级沙箱或 kruon 文件代理架构 | 中 | 中 | ❌ W2+ |
| 网络出口 | 暂缓（W1 不实现） | — | — | — | ❌ 候选暂缓，需产品负责人接受剩余风险 |
| 网络出口 | 审计日志（DNS 查询记录） | `unverified` | 中 | 低（仅审计） | ✅ 可选 |
| 环境变量 | 允许列表 + 拒绝模式清理 | `inferred` — 架构设计存在，env_redacted 字段已定义 | 低 | 高 | ✅ |

### 7.2 方案依赖关系

```
W1 实现顺序依赖:
  setpgid ──────────────────────────────────┐
                                            ├── 进程组残留检测
  环境变量清理 ──────────────────────────────┤
                                            │
  Policy Engine 事后检测 ────────────────────┤
                                            │
  网络审计日志（可选） ──────────────────────┘

W2+ 评估（不阻塞 W1）:
  macOS Sandbox / Seatbelt ─── 依赖 W1 的 setpgid + 进程组知识
  O_NOFOLLOW 受控路径 ──────── 与 Sandbox 一并评估
  pf / socketfilterfw ───────── 独立评估
```

---

## 8. P0 测试清单

### 8.1 进程组取消（覆盖 T-09）

| ID | 测试场景 | 预期 | 类型 | 优先级 |
|----|---------|------|------|--------|
| PT-01 | 单进程 CLI，SIGTERM 后在 5s 内退出 | 状态 `cancelled` | 集成 | P0 |
| PT-02 | 单进程 CLI，忽略 SIGTERM，5s 后 SIGKILL | 状态 `forced_stop_required` | 集成 | P0 |
| PT-03 | CLI 派生孙子进程，SIGTERM 到进程组后所有进程退出 | 无残留进程 | 集成 | P0 |
| PT-04 | CLI 派生孙子进程，SIGTERM 忽略，SIGKILL 后所有进程退出 | 无残留进程 | 集成 | P0 |
| PT-05 | 进程组中部分进程已僵尸，SIGKILL 后清理 | 无残留进程 | 集成 | P1 |
| PT-06 | 并行两个 Run，各自进程组独立取消 | 互不影响 | 集成 | P1 |

### 8.2 残留检测（覆盖 T-09）

| ID | 测试场景 | 预期 | 类型 | 优先级 |
|----|---------|------|------|--------|
| RT-01 | Run 结束后扫描进程组，无残留 | 检测通过 | 集成 | P0 |
| RT-02 | Run 结束后有残留进程（模拟孤儿进程） | 检测到残留，日志告警 | 集成 | P0 |
| RT-03 | 残留进程在检测前自然退出 | 检测通过（允许短暂窗口） | 集成 | P1 |

### 8.3 文件系统越界（覆盖 T-01, T-11）

| ID | 测试场景 | 预期 | 类型 | 优先级 |
|----|---------|------|------|--------|
| FT-01 | CLI 写入工作区内路径 | Policy Engine 允许 | 集成 | P0 |
| FT-02 | CLI 写入工作区外路径（`../../etc/cron.d/`） | Policy Engine 拒绝，终止后续动作，标记 failed | 集成 | P0 |
| FT-03 | CLI 写入绝对路径越界（`/etc/passwd`） | Policy Engine 拒绝，终止后续动作，标记 failed | 集成 | P0 |
| FT-04 | 路径含空格/特殊字符的工作区内路径 | 正确识别为工作区内 | 集成 | P1 |
| FT-05 | 写入前路径在工作区内，写入时路径变化（非 TOCTOU） | Policy Engine 重新检查后拒绝 | 集成 | P1 |

### 8.4 符号链接 TOCTOU（覆盖 T-02）

| ID | 测试场景 | 预期 | 类型 | 优先级 |
|----|---------|------|------|--------|
| ST-01 | 工作区内符号链接指向工作区内文件 | 允许写入 | 集成 | P0 |
| ST-02 | 工作区内符号链接指向工作区外文件 | Policy Engine 拒绝 | 集成 | P0 |
| ST-03 | TOCTOU：检查后、写入前替换符号链接目标 | Policy Engine 事后检测到越界，终止后续动作，标记 failed/unknown；**无法阻止已发生的写入** | 集成 | P0 |
| ST-04 | 符号链接链（link -> link2 -> /etc/passwd） | realpath 解析到最终目标，拒绝 | 集成 | P1 |
| ST-05 | 并发 TOCTOU：两个 Run 同时操作同一符号链接 | 至少一个被拒绝 | 集成 | P1 |

### 8.5 环境变量清理（覆盖 T-05）

| ID | 测试场景 | 预期 | 类型 | 优先级 |
|----|---------|------|--------|--------|
| ET-01 | 宿主环境含 `ANTHROPIC_API_KEY`，子进程不继承 | 子进程 env 中无该变量 | 集成 | P0 |
| ET-02 | 宿主环境含 `SSH_AUTH_SOCK`，子进程不继承 | 子进程 env 中无该变量 | 集成 | P0 |
| ET-03 | `PATH`/`HOME`/`USER` 等安全变量被继承 | 子进程 env 中有这些变量 | 集成 | P0 |
| ET-04 | `HTTP_PROXY` 被继承 | 子进程 env 中有代理设置 | 集成 | P0 |
| ET-05 | 允许列表外的变量被拒绝 | 子进程 env 中无意外变量 | 集成 | P1 |
| ET-06 | 拒绝模式匹配（`AWS_*`, `GITHUB_*`） | 子进程 env 中无匹配变量 | 集成 | P1 |

### 8.6 网络审计（覆盖 T-04，可选）

| ID | 测试场景 | 预期 | 类型 | 优先级 |
|----|---------|------|--------|--------|
| NT-01 | CLI 子进程发起 DNS 查询 | 查询被记录到审计日志 | 集成 | P1 |
| NT-02 | CLI 子进程连接 LLM API | 连接目标被记录 | 集成 | P1 |

---

## 9. 推荐实现顺序

### 9.1 W1 冲刺（当前迭代）

```
优先级 1 — 进程组管理（解决 T-09 核心问题）
  ├── setpgid(0, 0) 在子进程 fork 后
  ├── kill(-pgid, SIGTERM) → 5s → kill(-pgid, SIGKILL)
  └── 超时策略实现（5s cancelling → 10s forced_stop_required）

优先级 2 — 环境变量清理（解决 T-05 环境透传）
  ├── 定义 ALLOWED_ENV_KEYS 允许列表
  ├── 定义 DENIED_PATTERNS 拒绝模式
  ├── 在 Process Supervisor spawn 时构建清理后的 env
  └── 更新 LaunchPlan.env_redacted 记录清理结果

优先级 3 — 文件系统事后检测（解决 T-01/T-11 检测不阻止）
  ├── Policy Engine 在 artifact 事件到达时重新解析路径
  ├── 越界 → 终止后续动作，标记 failed/unknown
  └── 集成测试覆盖

优先级 4 — 残留进程检测（解决 T-09 残留）
  ├── Run 结束后扫描进程组
  ├── 检测到残留 → 日志告警 + 再次 SIGKILL
  └── 集成测试覆盖

优先级 5 — 网络审计日志（可选，T-04 缓解）
  ├── 记录 CLI 子进程的 DNS 查询
  └── 在诊断包中暴露
```

### 9.2 W2+ 评估

```
优先级 6 — macOS Sandbox / Seatbelt spike
  ├── 验证 sandbox-exec 在 macOS 26.5.1 上的可用性
  ├── 编写 Seatbelt profile（文件系统 + 网络）
  ├── 验证对 Codex/Claude CLI 的兼容性
  └── 决策：采用 / 降级 / 放弃

优先级 7 — O_NOFOLLOW 受控写入路径（与 Sandbox 一并评估）
  ├── 前提：所有写入经 kruon 文件代理/受控协议，或依赖系统级沙箱
  ├── 在受控路径上使用 O_NOFOLLOW 防止符号链接 TOCTOU
  └── 决策：采用 / 放弃

优先级 8 — 网络出口限制
  ├── 基于 W2+ 的 Sandbox 评估结果
  ├── 或 socketfilterfw 按进程控制
  └── 或 SOCKS5 代理方案
```

---

## 10. 失败回退策略

### 10.1 进程组管理失败

| 失败场景 | 回退 | 影响 |
|---------|------|------|
| `setpgid` 调用失败（权限/进程状态） | 回退到仅终止直接子进程（当前行为） | 残留风险升高，但取消仍能终止主进程 |
| CLI 自身调用 `setsid()` 创建新会话 | 检测 CLI 的进程组 ID，使用该 pgid | 需要额外 spike 验证 |
| SIGKILL 无法终止进程（内核 bug/不可杀进程） | 日志告警，标记 `forced_stop_required` | 用户手动终止 |

### 10.2 环境变量清理失败

| 失败场景 | 回退 | 影响 |
|---------|------|------|
| 允许列表过于严格导致 CLI 功能异常 | 扩展允许列表，保留拒绝模式的优先级 | 需要 spike 验证 CLI 依赖 |
| 拒绝模式遗漏新类型 secret | 依赖事件解析层的 `redact()` 事后脱敏 | 脱敏是事后防御，不能阻止泄露 |
| CLI 需要 SSH_AUTH_SOCK 进行 git 操作 | 将 SSH_AUTH_SOCK 加入允许列表（可选） | 需要在安全性和功能性间权衡 |

### 10.3 文件系统隔离失败

| 失败场景 | 回退 | 影响 |
|---------|------|------|
| Policy Engine 事后检测延迟过高 | 异步处理，不阻塞事件流 | 检测可能晚于写入完成 |
| macOS Sandbox 不可用/不兼容 | 维持 Layer 1（Policy Engine 事后检测） | 文件系统隔离效果降级：仅能事后检测，无法阻止写入 |
| O_NOFOLLOW 受控路径不可行（W2+） | 依赖 Sandbox 或放弃此层 | TOCTOU 防御依赖 Sandbox 内核级限制 |

### 10.4 总体回退

如果 W1 结束时以下任意一项无法通过 P0 测试：

1. **进程组取消**（PT-01 ~ PT-04 任一失败）→ 维持当前 `proc.terminate()` → `proc.kill()` 行为，标记 `forced_stop_required` 状态。进入 W2 前必须解决。
2. **环境变量清理**（ET-01 ~ ET-04 任一失败）→ 回退到空环境（仅传递 `PATH`/`HOME`），在 W2 迭代完善。
3. **文件系统事后检测**（FT-01 ~ FT-03 任一失败）→ 维持当前 `_path_in_workspace()` 标记行为，标记为已知剩余风险。

**不因任何单项失败而阻塞 W1 整体交付**，但必须在 W1 结束演示中展示每项的真实状态（通过/降级/推迟）。

---

## 11. 明确非目标

以下不在本文件范围内，也不在 W1 进程隔离实现范围内：

| 非目标 | 原因 | 处理 |
|--------|------|------|
| macOS Sandbox / Seatbelt profile 编写 | 需要 spike 验证兼容性后决策 | W2+ 评估 |
| O_NOFOLLOW 受控写入路径 | 依赖系统级沙箱或 kruon 文件代理架构 | W2+ 评估 |
| `pfctl` 或 `socketfilterfw` 网络规则 | 需要 root 权限，UX 负担高 | W2+ 评估 |
| 容器化隔离（Docker/ Lima） | 增加依赖和延迟，不适合桌面应用 | 不采用 |
| Linux 兼容性 | kruon MVP 为 macOS 桌面应用 | 不评估 |
| 进程资源配额（CPU/内存） | 威胁模型非目标（§5 非目标） | 不实现 |
| 文件系统完整性校验 | 威胁模型非目标 | 依赖 git |
| AI prompt 注入检测 | 威胁模型非目标 | 依赖审批机制 |
| 网络流量内容审计 | 超出 MVP 范围 | 不实现 |
| 跨用户隔离 | 威胁模型非目标（单用户桌面） | 依赖 macOS 用户隔离 |
| 防篡改二进制签名 | 威胁模型非目标（M4 计划） | 依赖 Gatekeeper |
| 全磁盘加密 | 系统级功能 | 依赖 FileVault |

---

## 附录 A: 引用

- `workspace-policy-threat-model-v1.md` — T-01 路径越界, T-02 符号链接/TOCTOU, T-04 网络外传, T-09 进程树/取消/残留, T-05 Secret 与日志泄漏
- `kruon_MVP开发计划_2026-07-11.md` — §3.2 模块边界, §3.4 审批不对称, §5.1 M0 退出标准, §6 适配器契约约束
- `spikes/cli-adapters/probe.py` — `execute_model_call()` 当前进程管理实现
- `spikes/cli-adapters/event_parser.py` — `_path_in_workspace()` 路径检查, `redact()` 脱敏
- `spikes/cli-adapters/capability_manifest.py` — `LaunchPlan.env_redacted` 字段, `build_launch_plan()` 启动计划构建
- `spikes/cli-adapters/README.md` — 已验证事实 vs 推断

## 附录 B: 证据等级定义

| 等级 | 含义 | 本文件示例 |
|------|------|-----------|
| `verified` | 有代码实现、单元测试或帮助文本证据 | `_path_in_workspace()` 路径规范化 |
| `inferred` | 架构设计有此能力，但未经过真实运行验证 | Policy Engine 事后检测架构 |
| `unverified` | 已识别为需求，但尚未实现或测试 | 网络审计日志 |
| `spike_needed` | 需要实机 spike 验证后才能决策 | setpgid CLI 兼容性、macOS Sandbox、proc_listchildpids |
