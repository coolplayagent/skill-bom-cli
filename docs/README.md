# Skill BOM CLI：使用与实现

这本书从一个可复现的项目安装开始，逐步说明声明、锁定、部署、审计以及开发本工具的方式。每一章都是普通 Markdown，可以直接在 GitHub 阅读，无需文档生成器。规范性需求仍以 [CodeSpec](../codespec/requirements/skill-bom-cli.md) 为准。

1. [入门](01-getting-started/README.md)：安装 CLI，创建首个声明，认识 CLI Skill 发布包。
2. [使用指南](02-user-guide/README.md)：配置来源、解析版本、安装与离线运行。
3. [参考手册](03-reference/README.md)：命令、文件格式、BOM 与错误处理。
4. [贡献者指南](04-contributor-guide/README.md)：模块边界、Bazel 缓存、发布及验证。

本工具只管理 Skill。它不会运行 Skill 正文中的命令，也不会安装 CLI、MCP 或 Skill 的运行依赖。没有结构化依赖元数据的 Skill 可以安装，但其依赖状态必须保留为“未知”。
