# 声明来源与包元数据

每个依赖都指定来源。ClawHub 需要注册表别名和 `@owner/slug`；Git 需要仓库地址，可带 Skill 子目录；HTTPS 归档需要稳定 URL、精确 SemVer 和完整 SHA-256。不要从搜索结果推断包身份。

```toml
schema_version = 1
[project]
name = "team-agent"
[install]
target = "./skills"
[registries.clawhub]
kind = "clawhub"
url = "https://clawhub.ai"
[dependencies.review]
registry = "clawhub"
package = "@team/review"
version = "^1.2"
[dependencies.release]
git = "https://github.com/team/skills.git"
subdir = "release"
version = "~2.1"
tag_pattern = "release-v{version}"
```

发布者可在 Skill 根目录加入 `skill.toml`，以 `[package]` 声明名称、版本、描述、许可证，以 `[dependencies.alias]` 声明直接 Skill 依赖。传递依赖中的 Registry 别名必须由根项目配置；远端包不能更改用户的 Registry 设置。未知字段和非法来源组合会被拒绝。

旧 Skill 仍可安装。若确知其完整依赖，在根声明中用 `[[package_metadata]]` 精确匹配来源和版本，并设 `complete = true`。补充声明进入锁文件和 BOM；它不能覆盖上游已有的 `skill.toml`。示例见[设计规范](../../codespec/design/skill-bom-cli.md)。

Registry 的 `token_env` 只指定环境变量名；凭据值不进入声明、锁或 BOM。Git SSH 使用本机 Git 凭据。归档必须使用 HTTPS（本机回环测试除外）。

[上一篇：使用指南目录](README.md) · [下一篇：版本解析](02-resolution.md)
