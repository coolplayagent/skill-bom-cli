# 版本解析与更新

`lock` 选择满足所有约束的确定版本和不可变内容。SemVer 支持精确、`^`、`~` 与比较范围；默认不选预发布，除非约束明确包含它。解析器按确定顺序优先取兼容的最高版本，并在必要时回溯。同一来源身份在一个环境中只能选一个版本。

```sh
skill-bom lock
skill-bom update
skill-bom update review
skill-bom tree
skill-bom why @team/review
```

已有锁的版本优先保留。`update` 升级全部包，`update review` 先释放该根依赖的锁偏好；其他节点只有在约束要求时才改变。Git `version` 从 tag 解析，默认 `v{version}`；`rev` 允许提交、tag 或 branch，但锁文件总是完整 commit。`rev` 与 `version` 互斥。ClawHub 的 `tag` 在锁定时解析为具体版本或固定快照；revision 快照不会伪装为 `0.0.0`。

AgentCenter 目前只提供最新版本候选，不枚举历史版本；详见[接入章节](04-agentcenter.md)。

若多个依赖给同一包施加冲突约束，命令会给出从根到冲突节点的链。循环依赖直接报错。目录名称相同但包身份不同属于部署冲突，不会自动重命名或覆盖。

[上一篇：来源声明](01-declarations.md) · [下一篇：安装](03-installation.md)
