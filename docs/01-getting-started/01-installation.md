# 安装与第一个项目

开发仓库固定 Rust 1.97.1、Bazel 9.2.0。Git 来源还要求本机有 Git。运行 `cargo build --locked` 或 `bazel build --lockfile_mode=error //:skill-bom`，生成的二进制分别位于 `target/debug/skill-bom` 和 `bazel-bin/skill-bom`。发布包中的 Skill 会按平台携带可执行文件。

在需要管理 Skill 的项目根目录运行：

```sh
skill-bom init
skill-bom validate
```

`init` 只创建最小 `skills.toml`，已有文件不会被覆盖。编辑声明后，以一个明确来源增加依赖。例如：

```toml
schema_version = 1
[project]
name = "my-agent"
[dependencies.review]
registry = "clawhub"
package = "@example/code-review"
version = "^1.2"
[registries.clawhub]
kind = "clawhub"
url = "https://clawhub.ai"
```

包名只是示例。运行 `skill-bom lock` 生成 `skills.lock`，再运行 `skill-bom install --locked`。CLI 会在 stderr 明确打印所用声明、锁文件和目标路径；默认项目目标是声明旁的 `skills/`。将 `skills.toml` 与 `skills.lock` 一起提交，其他机器才能安装相同的内容。

首次试用远端 Skill 前，可用 `skill-bom install --dry-run` 查看目录增删及冲突；这一步不会修改目标或锁文件，但可能填充内容缓存。

[上一篇：入门目录](README.md) · [下一篇：锁文件与安装](02-first-lock.md)
