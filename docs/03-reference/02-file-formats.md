# 声明、锁和安装记录

`skills.toml` 保存用户期望，`skill.toml` 保存发布者的包元数据。两者的依赖都使用显式来源模型。完整样例在 [examples/skills.toml](../../examples/skills.toml) 和 [examples/skill.toml](../../examples/skill.toml)。实现导出的 Schema 可通过 `skill-bom schema manifest|package|lock|bom|installed|error` 获取；仓库也保留对应的 [JSON Schema](../../schemas/manifest.schema.json)。

`skills.lock` 是稳定排序的 JSON，记录格式/解析语义版本、声明摘要、根与传递依赖边、包身份、精确版本或 revision、来源证据、内容清单和部署目录。它不包含凭据、临时签名 URL、用户名或绝对安装路径。不同包即使名称相同也保留不同身份，部署前必须解决目录冲突。

内容树摘要 `skill-tree-sha256-v1` 对排序后的相对路径、文件长度及文件 SHA-256 作固定编码；时间戳和压缩方式不参与。归档 SHA-256、ClawHub 上游摘要和本地树摘要各有不同范围，不能混用。文件权限在安装事实中另行记录。

AgentCenter 的锁定来源使用 Registry URL、稳定 `skillId` 和显式 `subdir` 作为身份；归档 SHA-256 是本地下载证据，不代表服务端签名或逐文件摘要。配置与版本限制见[使用指南](../02-user-guide/04-agentcenter.md)。

目标内 `.skill-bom/` 记录所有权、已部署包和事务状态。它是本机事实，不应当代替提交到仓库的锁文件。BOM 是显式导出物，也不隐式修改声明或锁。未知主版本的锁不会被自动降级。

[上一篇：命令](01-commands.md) · [下一篇：BOM](03-bom.md)
