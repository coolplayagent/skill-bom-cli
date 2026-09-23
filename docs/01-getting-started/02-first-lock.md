# 读懂锁文件与安装结果

`skills.toml` 表示意图，`skills.lock` 固定解析结果，安装目标中的 `.skill-bom/` 保存当前环境的部署事实。三者的职责不同。普通 `install` 保留锁定版本；声明变动后必须重新 `lock`，有锁但声明摘要不匹配时安装会失败。

```sh
skill-bom tree
skill-bom why code-review
skill-bom list
skill-bom verify
skill-bom bom --from lock --format json
```

`tree` 显示根依赖与传递依赖；`why` 显示到指定包的引入路径。`verify` 将目标内容、安装记录和当前锁文件比较。锁定 BOM 描述期望图，无需部署或网络；它不证明内容已经安装。安装视图 `bom --from installed` 会验证目录，发现漂移时仍可输出诊断 BOM，但退出码为 1。

Skill 如果只有 `SKILL.md` 而没有 `skill.toml`，其依赖元数据是 `unknown`。这不是“无依赖”。维护者可用精确来源和版本的 `[[package_metadata]]` 补充，或运行 `--strict-metadata` 拒绝未知项。

[上一篇：安装](01-installation.md) · [下一篇：CLI Skill](03-cli-skill.md)
