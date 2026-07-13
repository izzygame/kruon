# kruon 双 CLI 适配器 Spike

这是 W1 的能力探针与契约验证骨架，不是生产适配器。它用于验证 Codex CLI 与 Claude Code CLI 的版本、帮助信息、事件归一化、安全启动计划和 synthetic fixtures。

## 安全默认值

- 默认只运行 `codex/claude --version` 与 `--help`，不会调用模型。
- 真实模型调用必须同时显式提供 `--allow-model-call --execute`。
- 禁止生成或传递任何 `dangerously` / bypass 权限参数。
- 任务文本通过 stdin 传递，不放入进程参数列表。
- Codex 默认使用 `workspace-write` sandbox；Claude 默认声明 `manual` permission mode。
- 输出在持久化前经过基础 secret 脱敏；当前规则不替代正式诊断包的二次扫描。

## 快速运行

```bash
cd spikes/cli-adapters

# 安全能力探针：只读 version/help
python3 probe.py

# 验证全部 synthetic fixtures
python3 probe.py --fixtures-self-test --verbose

# 运行单元测试
python3 -W error::ResourceWarning -m unittest discover -s tests -v

# 仅查看冻结启动计划，不实际调用模型
python3 probe.py --allow-model-call \
  --tool codex \
  --task "列出工作目录文件" \
  --workspace "$PWD"
```

不要在普通验证中追加 `--execute`。真实调用会消耗额度，并且 Claude 双向 stream-json 输入形状、逐动作审批回传和 Codex 实验性服务协议仍需单独 spike。

## 文件结构

- `probe.py`：安全探针、fixture self-test、显式 opt-in 的实验调用入口。
- `capability_manifest.py`：能力声明、审批模式、危险参数拒绝和冻结启动计划。
- `capability_manifest.schema.json`：能力清单 schema（schema_version、adapter/tool/version、approval_mode、interfaces、capabilities 的 evidence level）。
- `adapter_protocol.py`：Adapter Protocol ABC，定义 12 个契约方法（probe、capabilities、prepare、start、send_input、stream_events、respond_approval、cancel、resume、collect_artifacts、reconcile、diagnostics）及其值对象。
- `event_parser.py`：Codex/Claude JSONL 到统一事件的容错解析与脱敏。
- `normalized_event.schema.json`：规范事件 schema。
- `mini_jsonschema.py`：本 spike 使用的无第三方依赖校验器。
- `snapshots/`：当前版本的能力快照（含 schema_version）。
- `evidence/`：本机 version/help 证据。
- `fixtures/`：10 组 synthetic JSONL，覆盖成功、畸形 JSON、未知事件、非零退出和取消/未知终态。
- `fixtures/live/`：W1 受控真实只读调用的脱敏归一化捕获；与 synthetic fixtures 分开，不参与 10/10 自测计数。
- `tests/`：unittest 套件（test_adapter_protocol.py、test_capability_manifest.py、test_capability_schema.py、test_event_parser.py、test_fixtures.py）。

## 验证命令

```bash
cd spikes/cli-adapters

# 运行全部单元测试（含契约、schema、parser、fixtures）
python3 -W error::ResourceWarning -m unittest discover -s tests -v

# 运行 fixture self-test（解析所有 fixtures 并验证 schema）
python3 probe.py --fixtures-self-test --verbose

# 验证两个能力快照符合 schema
python3 -c "
import json, sys; sys.path.insert(0, '.')
from capability_manifest import validate_snapshot_file
for name in ('codex', 'claude'):
    errs = validate_snapshot_file(f'snapshots/{name}_capability.json')
    print(f'{name}: {\"PASS\" if not errs else \"FAIL: \" + str(errs)}')
"
```

## 审批能力不对称

能力契约必须显式声明：

```text
approval_mode = per_action | sandbox_policy_only | none
```

- Codex `exec --json` 的帮助信息没有 `--ask-for-approval`，当前声明为 `sandbox_policy_only`。
- Claude Code 提供 `--permission-mode manual`、stream-json 和 hook 事件入口，当前目标声明为 `per_action`。
- 两者不能在 UI 中展示为等价能力。帮助信息只能证明参数存在，不能证明真实逐动作审批闭环已经成立。

## 已验证事实

- 探针采集时本机存在 Codex CLI 与 Claude Code CLI；版本和帮助文本证据保存在 `evidence/`，不等同于真实任务闭环验证。
- `codex exec` 支持 JSONL、sandbox、工作目录和配置隔离参数。
- `codex exec --help` 未声明逐动作审批参数。
- Claude Code 帮助信息声明 stream-json、manual permission mode、hook events、预算与会话恢复入口。
- 10/10 synthetic fixtures 能被解析并通过规范事件 schema。
- 危险参数拒绝、指纹漂移、未知事件降级、取消未知终态和 secret 脱敏均有自动化测试。

## 尚未验证的推断

- Claude `permission_request` 的真实事件形状及批准、拒绝、缩小参数的双向回传。
- Codex `app-server` / `exec-server` 是否提供稳定的逐动作审批协议。
- 两款 CLI 的真实取消时延、子进程残留和恢复语义。
- 当前 synthetic 事件字段是否覆盖真实版本的所有变化。
- Claude stream-json stdin 消息形状和会话生命周期仍需真实、受控调用验证。

在上述项目得到真实运行证据前，能力快照中的相关字段必须保留 `inferred` 或降级标记，不得进入生产承诺。
