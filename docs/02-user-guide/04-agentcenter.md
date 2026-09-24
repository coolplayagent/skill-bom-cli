# 接入 AgentCenter Registry

AgentCenter 是可选的内部 Skill 来源。你需要目标服务的访问权限、稳定的
`skillId` 和 W3 `X-Auth-Token`。在项目声明中填写 Registry 地址和保存令牌的
**环境变量名**，不要把令牌写进 TOML、锁文件或命令行参数。

```toml
schema_version = 1

[project]
name = "team-agent"

[registries.market]
kind = "agentcenter"
url = "https://agent.huawei.com"
token_env = "AGENTCENTER_W3_TOKEN"

[dependencies.review]
registry = "market"
package = "review-skill-id"
version = "=1.2.3"
```

先在当前 shell 中设置 `AGENTCENTER_W3_TOKEN`，再运行 `skill-bom validate`、
`skill-bom lock` 与 `skill-bom install --locked`。CLI 使用 `X-Auth-Token`
请求详情和 ZIP；认证请求不跟随重定向，网络 TLS 验证保持开启。ZIP 在部署前
经过安全解压与内容校验。锁文件记录归档 SHA-256 和本地内容树摘要；之后
`install --frozen` 可从完整缓存离线安装。

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
描述的协议开发。本仓库没有内部服务凭据，普通测试只运行本地 HTTP 契约
fixtures；在真实环境使用前，应先以授权的测试 Skill 验证服务响应。
协议细节与验证边界见 [CodeSpec](../../codespec/design/agentcenter.md)。

[上一篇：安装、离线与漂移](03-installation.md) · [返回使用指南目录](README.md)
