# 测试与证据

普通测试使用临时 Git 仓库、本地 Rust HTTP 服务和隔离目标，不依赖公网，也不改变用户 Git 配置。验收矩阵在 [CodeSpec 测试契约](../../codespec/test/skill-bom-cli.md)。ClawHub 契约 fixtures 固定在上游源码基线；在线 smoke 单独运行并记录服务可用性。

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
