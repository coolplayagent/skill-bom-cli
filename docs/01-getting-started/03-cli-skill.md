# CLI Skill 发布包

仓库中的 [Skill 定义](../../skills/skill-bom-cli/SKILL.md) 是包裹本 CLI 的 Agent 操作入口。发布时，同一个归档含 `SKILL.md`、`skill.toml` 和各平台的 `assets/<OS>/<ARCH>/skill-bom`（Windows 为 `.exe`）。Skill 给 Agent 提供何时调用哪个命令的判断；实际解析和部署由可执行文件完成。

可把发布归档解压到 Agent 的 Skill 目录。Skill 会优先选择归档中匹配本机的平台二进制；若没有匹配项，使用已安装在 `PATH` 的 CLI 或本仓库的锁定 Cargo 构建。它不会自行下载另一个 CLI，也不会执行被管理 Skill 的正文。

需要同时升级并安装项目依赖时，使用 `sync [alias]`；`sync --dry-run` 预览安装变更。它在成功部署后写锁，不能离线运行。

源码目录本身也含有 `SKILL.md` 和 `skill.toml`，但不含构建产物。若直接安装该目录，使用者应另行提供 CLI。发布流程在创建归档前检查 Skill 与 Cargo 的版本一致，并验证预期平台二进制都已放入归档。

[上一篇：锁文件与安装](02-first-lock.md) · [下一篇：使用指南](../02-user-guide/README.md)
