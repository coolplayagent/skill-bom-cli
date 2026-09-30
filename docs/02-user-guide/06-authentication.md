# 按 origin 管理登录账号

从 0.0.7 起，每个 origin 可以独立保存一个账号。AgentCenter 的 W3 账号、
ClawHub 的账号和自定义 Registry 的账号可以同时保留；重新登录只替换指定
origin 的账号，退出也只清理该 origin。升级前已有的 AgentCenter 登录继续可用。

origin 是协议、主机和端口，例如 `https://agent.huawei.com` 或
`https://registry.example:8443`，不能包含账号、路径、查询或片段。
主机名大小写、默认 HTTPS 443 端口和末尾 `/` 归一化；不同端口分别管理。
ClawHub 的 Registry URL 可以有基础路径，但登录仍按它的 origin 选择账号。
远端必须使用 HTTPS；回环 HTTP 只用于本地测试。

## 登录和账号选择

```sh
# 官方 AgentCenter：隐藏输入 W3 密码
skill-bom auth login --origin https://agent.huawei.com --username your-w3-account

# ClawHub 或自定义 Registry：隐藏输入该站点 Token
skill-bom auth login --origin https://clawhub.ai --username your-clawhub-account
skill-bom auth login --origin https://registry.example --username service-account

# 查看所有账号，然后只退出 ClawHub
skill-bom auth status --all --format json
skill-bom auth logout --origin https://clawhub.ai
```

三个子命令都无需项目配置。省略 `--origin` 时仍然指向官方 AgentCenter，
不会根据当前项目或最近一次登录改变默认值。`status --origin URL` 查看单个站点。
同一 origin 内需要换账号时重新登录即可，本版本不保留同站点的多个历史账号。

非交互 W3 登录必须同时使用 `--username NAME --password-stdin`；其它 origin
使用 `--username NAME --token-stdin`。通过安全输入源向 stdin 提供秘密，
不要把密码或 Token 放入命令参数、聊天、TOML、锁文件或 BOM。两个 stdin
选项互斥，且必须匹配登录方式。输入限制为账号 256 字节、密码 4096 字节、
Token 16384 字节；Token 需为非空白可打印 ASCII。stdin 只去除行末换行。

Token 登录仅保存凭据和用户提供的账号标签，不验证账号归属或服务端有效性，
因此可离线执行。CLI 明确显示有效性未验证、过期时间未知；不会伪造四小时
有效期或自动刷新 Token。Token 被拒后，从该服务取得新 Token 再登录。
官方 AgentCenter 的密码交换、过期和刷新规则见[W3 登录](04-agentcenter.md)。

## 使用与隔离

AgentCenter 和 ClawHub 都先使用 Registry 配置中非空的 `token_env` 值，
再查找当前 origin 的本地凭据。显式 Token 被拒时不会切换到本地账号。
ClawHub 没有任何凭据时继续访问公开资源；AgentCenter 必须有可用凭据。
AgentCenter 使用 `X-Auth-Token`，ClawHub 使用 Bearer；跨 origin 重定向及
GitHub 下载交接均不携带原站点凭据。Git 自身认证和普通 HTTPS 归档不接入此账号库。

密码和 Token 保存在 Windows Credential Manager、macOS Keychain 或 Linux
Secret Service；普通元数据仅记录 origin、账号、方式、时间和凭据引用。
凭据库不可用时明确报错，不回退明文。不同 origin 使用独立命名空间和进程锁，
不同 `SKILL_BOM_HOME` 也隔离。一个站点登录、刷新、退出不会删除另一个站点的账号。

`status` 只读本地元数据，不联网、不读系统凭据库，也不刷新。
JSON 包含 `origin`、`method`（`w3` 或 `token`）、`logged_in`、`username`、
`expires_at`、`expiry_source`、`expired` 和 `cleanup_pending`。
Token 的三个过期字段为 null；`logged_in` 表示本地有记录，不证明服务端有效。
`status --all` 返回按 origin 排序的 `sessions` 数组，包括已保存登录和待清理记录。

退出先撤销本地会话再清理秘密；清理失败会保留可重试的记录并返回失败。
再次对该 origin 执行 logout 完成清理。退出不影响显式环境变量或其它应用的登录态。

[返回使用指南](README.md) · [命令与退出码](../03-reference/01-commands.md)
