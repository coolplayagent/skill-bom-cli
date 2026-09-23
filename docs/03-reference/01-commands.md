# 命令与退出码

| 命令 | 用途 |
| --- | --- |
| `init` | 创建最小声明，不覆盖已有文件 |
| `validate` | 校验 TOML、来源组合和配置引用 |
| `lock` | 解析依赖图并写锁，不部署 |
| `update [alias]` | 请求升级全部或一个根依赖 |
| `install` | 按锁部署；无锁时创建锁 |
| `tree` / `why <package>` | 查看图及引入路径 |
| `list` / `verify` | 查看安装记录及内容漂移 |
| `bom` | 导出锁定或安装 BOM |
| `schema <kind>` | 导出版本化 JSON Schema |

通用选项有 `--manifest PATH`、`--global`、`--target PATH`、`--offline`、`--strict-metadata`、`--format json`。`--format spdx-json` 只适用于 `bom`。`install --locked` 要求匹配的锁；`install --frozen` 还禁止网络；`install --dry-run` 输出计划而不修改目标或锁。

`bom --from lock` 是默认视图；`bom --from installed` 验证安装。`--timestamp RFC3339` 或 `SOURCE_DATE_EPOCH` 可固定生成时间。运行 `skill-bom --help` 和各子命令的 `--help` 可查看当前二进制接受的完整参数。

退出码 `0` 表示完成，允许明确列出的警告；`1` 表示确定失败或漂移；`2` 表示无效输入、网络/缓存不足等未完成操作；`130` 表示用户中断。`--format json` 的成功结果写 stdout，日志和结构化错误写 stderr。错误包含稳定代码、阶段、包身份、依赖链和修复提示。

[上一篇：参考目录](README.md) · [下一篇：文件格式](02-file-formats.md)
