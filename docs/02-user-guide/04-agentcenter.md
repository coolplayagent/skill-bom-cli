# 接入 AgentCenter Registry

AgentCenter 是可选的内部 Skill 来源。你需要目标服务的访问权限、稳定的
`skillId`。先运行 `skill-bom auth login`，交互输入 W3 账号和密码；密码不回显。
登录成功后，CLI 为 `https://agent.huawei.com` 自动注入 `X-Auth-Token`。
首期只管理 skill-bom 自身的单个账号，不读取其他应用的登录态。

```toml
schema_version = 1

[project]
name = "team-agent"

[registries.market]
kind = "agentcenter"
url = "https://agent.huawei.com"
# token_env = "AGENTCENTER_W3_TOKEN"  # 可选的显式覆盖

[dependencies.review]
registry = "market"
package = "review-skill-id"
version = "=1.2.3"
```

登录后运行 `skill-bom validate`、`skill-bom lock` 与 `skill-bom install --locked`。
`validate` 不检查登录态。CLI 使用 `X-Auth-Token`
请求详情和 ZIP；认证请求不跟随重定向。TLS 设置见下文。ZIP 在部署前
经过安全解压与内容校验。锁文件记录归档 SHA-256 和本地内容树摘要；之后
`install --frozen` 可从完整缓存离线安装。

```sh
skill-bom auth login
skill-bom auth status --format json
skill-bom auth logout
```

这三个命令无需项目声明，支持 text/json 输出。`auth status` 只读取本地记录，
显示账号、过期时间、是否已过期，以及时间来自服务端（`server`）还是本地
策略（`local_policy`）；它不联网验证、不读取凭据库、不刷新 token。
`auth logout` 可重复执行；删除失败会报错并保留待清理引用，再次退出可重试。
退出不影响显式环境变量，也不影响完整缓存的离线安装。

脚本登录必须同时提供 `--username NAME --password-stdin`，把密码从安全输入
管道传入 stdin。不要把密码放进命令参数、环境变量或配置文件。默认交互登录
允许只提供 `--username`。`--offline auth login` 会报错。

密码和 token 保存到 Windows Credential Manager、macOS Keychain 或 Linux
Secret Service，供后续进程自动重新认证；普通文件只保存账号、时间、凭据
引用和会话版本。命名空间按用户配置目录隔离（也包括 `SKILL_BOM_HOME`）。
Linux 没有可用或已解锁的 Secret Service 时返回 `AUTH_STORE_UNAVAILABLE`，
不会退回明文文件。CI 可继续使用显式 `token_env`。

配置的环境变量只要非空就优先使用；变量缺失或为空时，使用持久化登录态。
显式 token 被拒后，CLI 不刷新、不切换账号、不覆盖环境变量。自定义 Registry
仍要求显式 token，自动获得的 token 只会发送到 `https://agent.huawei.com`。
密码只发送到固定 HTTPS secureLogin 端点；登录和 AgentCenter 请求均拒绝
重定向。不要把令牌写进 TOML、锁文件、BOM 或参数。

## 内网 TLS 证书与排错

从 0.0.5 起，`AGENTCENTER_VERIFY_TLS` 同时控制固定 W3 登录端点和所有
AgentCenter Registry 的详情、下载请求（包括自定义 Registry）。未设置时
默认为 `false`，兼容 [Issue #4](https://github.com/coolplayagent/skill-bom-cli/issues/4)
报告的华为内网证书链不完整问题。自动重新登录也使用同一设置。

`false` 会关闭服务器证书链和主机名验证：HTTPS 仍加密传输，但不能确认
服务器身份，密码、token 和下载内容可能遭到中间人截获或篡改。仅在信任的
内网环境使用兼容模式。可显式设为 `true` 强制验证；验证失败直接报错，
不会自动降级或按认证失败刷新凭据。ClawHub、普通 HTTPS 归档和 Git 的
证书验证不受这个变量影响。

```sh
# Linux / macOS：强制验证，作用于当前 shell 后续登录、lock、sync 等命令
export AGENTCENTER_VERIFY_TLS=true
skill-bom auth login
```

```powershell
# Windows PowerShell
$env:AGENTCENTER_VERIFY_TLS = "true"
skill-bom auth login
```

需恢复内网兼容模式时，把值改成 `false`。还支持 `1/0`、`yes/no`、`on/off`，
忽略大小写和首尾空白。空字符串、拼写错误或非 UTF-8 值会返回 `CONFIG`，
不会被当成 `false`。设置在 HTTP 客户端创建时读取，不写入锁文件或会话记录。
严格模式沿用 rustls 的标准根证书；此版本没有切换到系统证书库，也不会自动
导入 Windows/macOS 的内网证书。证书链不受信任时，应由服务维护者修复链或
在受信任内网明确选择兼容模式。

`NETWORK_TLS` 表示证书验证、TLS 握手或协议失败；先检查服务证书链、主机名、
代理及上述设置。`NETWORK_TIMEOUT` 表示请求超时，`NETWORK` 表示其他连接
或响应读取失败。错误不会输出密码、token、原始 URL 或服务端回显正文。

CLI 从已约定的整数秒或 RFC3339 字段取最早有效期；缺少可可靠解析的字段时，
采用本地 4 小时生命周期，在过半时重新登录。HTTP 400/401/403、空响应、JSON
`null` 或业务认证错误会触发至多一次刷新，详情 GET 与下载 POST 最多重试一次。
刷新失败会提示 `skill-bom auth login`；登录失败保留原会话。并发刷新会合并，
退出与刷新共享进程锁，退出后的旧请求无法重新发布登录态。

`package` 是服务端的稳定 `skillId`，不是显示名称。若 ZIP 根目录不是 Skill 根，
可在依赖中加 `subdir = "明确的/包根"`；工具不会猜测第一份 `SKILL.md`。
一个项目中的相同 Registry、skillId 和子目录只选一个版本。

目前报告的详情接口只给出最新版本，因此**新锁定只能选择该最新的 SemVer
版本**。`^`、`~` 或比较范围只有在最新版本满足约束时才成功；它们不会列举
历史版本。已有锁记录的旧版可凭归档摘要重新下载和复核；没有锁时，历史
版本不可验证就会报 `SOURCE_VERSION_UNAVAILABLE`。服务端若缺少
`skill.toml`，依赖元数据仍是“未知”；可使用精确补充声明，或用
`--strict-metadata` 拒绝它。

这份实现根据 [Issue #2](https://github.com/coolplayagent/skill-bom-cli/issues/2)
及 [Issue #3](https://github.com/coolplayagent/skill-bom-cli/issues/3)
描述的协议开发，并按 Issue #4 修复 TLS 兼容性。本仓库没有内部服务凭据，
普通测试运行本地 HTTP/HTTPS 契约 fixtures，覆盖自签名证书的兼容与严格模式；
在真实环境使用前，应先以授权的测试 Skill 验证服务响应。
协议细节与验证边界见 [CodeSpec](https://github.com/coolplayagent/skill-bom-cli/blob/main/codespec/design/agentcenter.md)
及 [W3 认证设计](https://github.com/coolplayagent/skill-bom-cli/blob/main/codespec/design/w3-auth.md)。

[上一篇：安装、离线与漂移](03-installation.md) · [返回使用指南目录](README.md)
