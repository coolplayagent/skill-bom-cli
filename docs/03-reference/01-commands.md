# 命令与退出码

| 命令 | 用途 |
| --- | --- |
| `init` | 创建最小声明，不覆盖已有文件 |
| `validate` | 校验 TOML、来源组合和配置引用 |
| `lock` | 解析依赖图并写锁，不部署 |
| `update [alias]` | 请求升级全部或一个根依赖 |
| `sync [alias]` | 升级允许的版本、校验完整依赖图并部署，成功后写锁 |
| `install` | 按锁部署；无锁时创建锁 |
| `tree` / `why <package>` | 查看图及引入路径 |
| `list` / `verify` | 查看安装记录及内容漂移 |
| `bom` | 导出锁定或安装 BOM |
| `schema <kind>` | 导出版本化 JSON Schema |
| `auth login [--origin URL] [--username NAME] [--password-stdin\|--token-stdin]` | 官方 AgentCenter 登录 W3；其它 origin 保存账号和 Token |
| `auth status [--origin URL\|--all]` | 查看指定 origin 或全部本地登录记录；无需项目配置 |
| `auth logout [--origin URL]` | 只清理所选 origin 的凭据；省略 origin 时为官方 AgentCenter |

通用选项有 `--manifest PATH`、`--global`、`--target PATH`、`--agent universal|codex|claude-code|cursor|relayagent`、`--offline`、`--strict-metadata`、`--format json`。`--format spdx-json` 只适用于 `bom`。`install --locked` 要求匹配的锁；`install --frozen` 还禁止网络；`install --dry-run` 输出计划而不修改目标或锁。

`sync` 默认升级所有根依赖及可升级的传递依赖；`sync alias` 沿用 `update alias` 的定向规则，别名必须是根依赖。`sync --dry-run` 显示新增、替换、删除和冲突，允许填充内容缓存，但不改锁或目标。`--offline sync` 报错，因为升级必须查询候选版本。实际同步先验证全部内容，再使用安装事务部署，部署成功后才写 `skills.lock`。若最后写锁失败，按错误提示重新运行 `sync` 以对齐锁与安装记录。

`bom --from lock` 是默认视图；`bom --from installed` 验证安装。`--timestamp RFC3339` 或 `SOURCE_DATE_EPOCH` 可固定生成时间。运行 `skill-bom --help` 和各子命令的 `--help` 可查看当前二进制接受的完整参数。

退出码 `0` 表示完成，允许明确列出的警告；`1` 表示确定失败或漂移；`2` 表示无效输入、网络/缓存不足等未完成操作；`130` 表示用户中断。`--format json` 的成功结果写 stdout，日志和结构化错误写 stderr。错误包含稳定代码、阶段、包身份、依赖链和修复提示。

认证错误包括 `AUTH_INPUT`（输入方式或内容无效）、`MISSING_AUTH_TOKEN`
（登录响应缺少 token，或凭据提供器属于另一个 origin）、`AUTH_REQUIRED`
（未登录、会话失效或认证被拒）及 `AUTH_STORE_UNAVAILABLE`（凭据库、元数据
或进程锁不可用）。均以退出码 2 返回；刷新过程的网络/协议错误保留其代码，
并附带 `skill-bom auth login` 指引。退出清理失败不会输出成功结果。
完整契约见[错误代码](https://github.com/coolplayagent/skill-bom-cli/blob/main/codespec/design/errors.md)。

认证的 origin 默认 `https://agent.huawei.com`，不从项目 Registry 别名推断。
Token 保存支持离线，但不验证远端有效性；其状态的过期字段为 null。
`status --all --format json` 返回按 origin 排序的 `sessions` 数组。
完整输入、优先级和隔离规则见[按 origin 登录](../02-user-guide/06-authentication.md)。

`NETWORK_TLS` 表示证书验证或 TLS 握手/协议失败，`NETWORK_TIMEOUT` 表示超时，
其余连接/读取失败使用 `NETWORK`。W3 与 AgentCenter 的
`AGENTCENTER_VERIFY_TLS` 默认 `false`，可设为 `true` 强制验证；无效值使用
`CONFIG`。范围、风险和平台设置见 [AgentCenter TLS 说明](../02-user-guide/04-agentcenter.md#内网-tls-证书与排错)。

[上一篇：参考目录](README.md) · [下一篇：文件格式](02-file-formats.md)
