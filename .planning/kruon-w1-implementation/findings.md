# kruon W1 实施发现

- 当前工作区尚未初始化产品代码仓库。
- 已确认 Claude Code CLI 2.1.205 可用于受限目录写入和支线执行。
- 本次不让 Claude 修改核心领域模型、PRD、开发计划或品牌资料。
- Claude 完成能力探针、规范事件 schema、解析器、能力快照、10 组 synthetic fixtures 和 unittest 骨架；两次因预算上限停止，未发生目录越界。
- Codex 独立审查发现并修复：任务文本被误当 CLI 参数、Bearer/env secret 脱敏边界、未关闭测试文件、Codex 顶层帮助与 exec 帮助混合导致的能力误报。
- 冻结启动计划现将任务经 stdin 传递，prompt 不出现在进程参数列表中。
- 最终安全探针确认 Codex exec `has_ask_for_approval=false`，Claude permission choices 六项均能正确提取。
- 最终验证：60 个 unittest 通过，10/10 fixtures 通过，py_compile 通过。
