# 安装、离线与漂移

项目与用户全局作用域独立。项目默认把 Skill 复制到声明旁的 `skills/`；`--global` 选用用户标准配置、缓存和数据目录。`--target PATH` 覆盖本次目标，但一个目标只能由一个环境管理。不要让另一个 Agent 同时读取正在替换的多个 Skill 目录。

```sh
skill-bom install --locked
skill-bom --offline install --locked
skill-bom install --frozen
skill-bom install --dry-run
skill-bom verify
```

`--frozen` 等同于锁定且离线。离线只使用已经验证的缓存；缺失或损坏会失败，不访问网络。在线缓存损坏会重新获取并按锁摘要验证。安装先取得和验证全部内容，再在目标文件系统暂存；独占锁、备份和事务日志支持失败回滚与下次写操作恢复。

非本工具管理的目录不会被覆盖。本工具管理的 Skill 若被用户修改，也会阻止替换或删除；先审查本地修改，再决定如何处理。`list` 显示安装记录，`verify` 检测缺失、修改和与当前锁文件的差异。删除只作用于本环境以前管理、现在不再需要且未被修改的包。

[上一篇：版本解析](02-resolution.md) · [下一篇：AgentCenter](04-agentcenter.md)
