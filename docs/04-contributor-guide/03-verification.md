# 测试与证据

普通测试使用临时 Git 仓库、本地 Rust HTTP 服务和隔离目标，不依赖公网，也不改变用户 Git 配置。验收矩阵在 [CodeSpec 测试契约](https://github.com/coolplayagent/skill-bom-cli/blob/main/codespec/test/skill-bom-cli.md)。ClawHub 契约 fixtures 固定在上游源码基线；在线 smoke 单独运行并记录服务可用性。

AgentCenter 的 HTTP fixtures 覆盖 `X-Auth-Token`、详情身份、ZIP 下载、限流、业务认证错误、离线缓存与端到端 CLI。其协议形状来自 [Issue #2](https://github.com/coolplayagent/skill-bom-cli/issues/2)；内部服务源码和凭据不在本仓库，尚无真实 AgentCenter 在线 smoke 证据。新锁定只使用详情接口报告的最新版本。

W3 认证的显式测试目标为 `auth`、`auth_protocol`、`auth_process`、`auth_cli`，
均已列入 CI、Bazel 和 Qualitygate。测试使用替身存储、可控时钟、临时目录和
独立子进程；协议依据 [Issue #3](https://github.com/coolplayagent/skill-bom-cli/issues/3)。
Linux 凭据库失败测试使用不存在的临时 D-Bus 地址，不访问真实登录态。
系统凭据库适配器使用官方 mock 测试；真实平台后端及 W3 在线登录需另行记录。

`auth_origins` 补充多 origin 并存、独立替换/退出、Token 元数据和输入、凭据
优先级、ClawHub 匿名与保存 Token、自定义 AgentCenter 请求和损坏状态隔离。
CLI 测试覆盖按 origin 查询、全部账号排序和离线 Token 保存的失败诊断；
进程测试确认 W3 锁不会阻塞另一个 origin。所有秘密都是隔离 fixtures。

Issue #4 的 TLS 回归由 Rust 本地 HTTPS 服务和临时自签名证书验证：默认兼容、
强制验证拒绝、测试专用可信根成功、认证重定向拒绝、超时分类及普通归档的
严格验证。CLI 子进程测试环境变量的实际生效；不修改进程全局环境或系统证书库。
这些证据不等同于华为内网服务实测。

```sh
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --lib --bins --all-features
cargo test --locked --tests --all-features
cargo llvm-cov --locked --all-targets --all-features --fail-under-lines 90
bazel test --lockfile_mode=error //...
```

夜间 Miri 只检查纯领域单元测试；ASan 在独立目标目录运行原生单元测试。`tests/quality.rs` 检查模块无环、I/O 所有权、文件规模、示例/Schema 与文档导航。最后用 Qualitygate 的 `full` profile 对交付快照执行检查，保存快照与策略摘要、警告和未完成项。若某平台或在线服务未运行，明确保留验证缺口。

[上一篇：构建与发布](02-build-and-release.md) · [返回全书目录](../README.md)
