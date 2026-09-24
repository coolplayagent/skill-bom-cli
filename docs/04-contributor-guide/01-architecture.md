# 模块与数据流

CLI 二进制只负责参数和中断处理；可复用库拥有领域、配置、解析、来源、缓存、安装与 BOM。`domain` 定义纯数据和校验，`resolver` 只依赖标准化候选/元数据，`sources` 获取 Git、HTTPS 归档、ClawHub 及 AgentCenter 内容，`store` 验证内容树，`installer` 管理目标事务，`application` 编排命令。

```text
skills.toml → config → resolver ⇄ sources → store
                           ↓                 ↓
                       skills.lock → installer → .skill-bom/
                           ↓                 ↓
                           └─────── bom ─────┘
```

依赖身份来自来源位置与包定位，不来自本地别名。来源适配器只负责候选、不可变来源和内容；它们不能解释 Agent 的查找优先级，也不能执行 Skill 正文。网络、Git 子进程、路径和环境变量有集中的边界。

Bazel 把共享基础模块、归档适配器、Git 适配器、ClawHub 适配器、AgentCenter 适配器、来源编排分别编译为 `rust_library`。Cargo 仍从 `src/lib.rs` 组合相同源码；Bazel 入口文件只重导出模块。修改 Git 适配器时，其他适配器与基础模块的编译动作仍可命中缓存。

[上一篇：贡献者目录](README.md) · [下一篇：构建与发布](02-build-and-release.md)
