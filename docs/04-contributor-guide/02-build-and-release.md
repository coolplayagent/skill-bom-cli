# 构建缓存与发布

Cargo manifest/lock 是 Rust 依赖图来源；Bazel `rules_rust` 的 crate_universe 消费同一锁。仓库固定 Rust、Bazel 和 rules_rust 版本。执行：

```sh
cargo build --locked
bazel mod deps --lockfile_mode=error
bazel build --lockfile_mode=error //:skill-bom
bazel test --lockfile_mode=error //...
```

`src/sources/BUILD.bazel` 有 `archive`、`git`、`clawhub`、`agentcenter`、`sources` 五个 `rust_library`；根 `BUILD.bazel` 的 `foundation` 与 `skill_bom` 组成其余模块。每个库只列自己编译的源码和依赖；不要把 `src/sources/*.rs` 重新放入根库的 `srcs`，否则适配器变动会扩大失效范围。

`skills/skill-bom-cli` 是发布单元的源码。`//:skill-package` 在 Linux x86_64 上制作单平台预览包。版本 tag `vX.Y.Z` 触发正式发布流程，先完成格式与 Cargo 测试，并在 Linux、macOS、Windows 构建本机 CLI；之后才将 `SKILL.md`、`skill.toml` 与各平台二进制组合为一个 `skill-bom-cli-skill-vX.Y.Z.tar.gz`，附 SHA-256。工作流检查 tag、Cargo 和 Skill 版本一致，并在发布前检查归档内容。仓库不在普通构建时偷偷下载 CLI；源码 Skill 需要已有 CLI 或本地构建。

跨平台发布包要在对应 runner 上执行二进制 smoke test。不要把某一平台的构建成功描述为所有平台已经运行验证。

文档站使用 mdBook 0.5.4 和根目录的 `book.toml`。本地运行 `mdbook build`，
输出位于 `target/book/`。`docs/SUMMARY.md` 声明全部章节；Rust 文档检查验证
目录覆盖、内部链接和指向仓库文件的链接。`scripts/check-book.rs` 在发布前检查
生成 HTML 的本地链接与静态资源。文档外的源码、Schema 和规范使用 GitHub
文件链接；`book.toml` 保留 README 页面重定向，避免站点导航失效。

`.github/workflows/pages.yml` 在文档变更或手动触发时构建站点，校验官方
mdBook 归档的固定 SHA-256，再通过 GitHub Pages Actions 部署。
Pull request 只构建；`main` 部署至
[在线文档](https://coolplayagent.github.io/skill-bom-cli/)。仓库 Pages 的构建来源
应配置为 GitHub Actions。

[上一篇：架构](01-architecture.md) · [下一篇：验证](03-verification.md)
