# Agent Skills 格式与安装目录

安装同时检查 [Agent Skills 格式](https://agentskills.io/specification)和部署位置。
每个 Skill 使用 `<name>/SKILL.md`；安装器保持正文与资源原文，不执行指令、
hooks、脚本或包安装步骤。`scripts/`、`references/`、`assets/` 和
`agents/openai.yaml` 等文件随完整内容树一起安装。

## 选择 Agent

| `--agent` / `install.agent` | 项目目录 | 用户全局目录 |
| --- | --- | --- |
| `universal`（默认） | `.agents/skills/` | `~/.agents/skills/` |
| `codex` | `.agents/skills/` | `~/.agents/skills/` |
| `claude-code` | `.claude/skills/` | `~/.claude/skills/` |
| `cursor` | `.cursor/skills/` | `~/.cursor/skills/` |
| `relayagent` | `.skills/` | `~/.relay/skills/` |

项目路径以 `skills.toml` 所在目录为基准；全局路径以用户主目录为基准。
共享目录是[集成约定](https://agentskills.io/integrate-skills)，并非格式标准强制指定的目录。
Codex、Claude Code 和 Cursor 分别依据其
[官方发现规则](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills)、
[官方目录表](https://code.claude.com/docs/en/skills#choose-where-skills-load)及
[官方 Skill 文档](https://cursor.com/docs/skills#skill-directories)。RelayAgent 使用本项目确认的目录契约。

```sh
skill-bom init --agent relayagent
skill-bom lock
skill-bom install --locked
skill-bom verify
skill-bom --global init --agent relayagent
skill-bom --global lock
skill-bom --global install --locked
```

`init --agent` 将预设保存到 `[install].agent`。也可以为单次命令传入
`--agent`，例如 `skill-bom install --agent claude-code --locked`；后续
`list`、`verify` 和 `bom --from installed` 使用相同选择。一次命令只有一个目标，
可重复命令安装到不同 Agent。Codex 和 universal 共享同一目标。

优先级为命令行、声明文件、默认 universal。同一层不能同时设置 agent 和 target。
`--target` 相对调用目录，`install.target` 相对声明文件；自定义目录的 JSON
结果中 `agent` 为 null。安装和预览结果包含 `agent`、`target`、`changes`、`conflicts`。

旧 `skills/` 或旧全局数据目录不会被搬动或删除。默认命令提示发现的旧安装；
使用 `--target` 或 `install.target` 显式管理它。更改声明后应重新运行 `lock`。
CLI 参数覆盖本身不改声明或已有锁。`SKILL_BOM_HOME` 将 Agent 主目录隔离到
该目录的 `home/`，适用于测试；它不代表真实 Agent 会扫描此隔离目录。

## 标准文件

```yaml
---
name: code-review
description: Review code changes when a user requests a code review.
license: MIT
metadata:
  version: "1.0"
---
```

入口必须精确命名为 `SKILL.md`，frontmatter 必须是闭合的 YAML 映射。
名称遵循 Unicode/NFKC 小写字母、数字及内部单连字符规则，长度为 1–64 字符；
安装目录采用规范化名称，并保留跨平台路径限制。描述必须非空且至多 1024 字符。
可选 compatibility 为 1–500 字符，license 和 allowed-tools 为字符串，metadata
为字符串到字符串映射。引号、多行内容、注释及 CRLF 均受支持。

额外客户端字段保留原文，不赋予安装器任何执行权限。Claude Code 的
`synced`、`anthropic-skills` 保留名称在对应预设下拒绝安装。
安装器限制 `SKILL.md` 为 1 MiB、frontmatter 为 64 KiB，并限制解析深度、
节点和别名展开；外部文件包含与变量插值不启用。

`skill.toml` 和精确来源补充仍是依赖声明协议；名称必须与标准入口一致。
描述取自标准入口，许可证优先采用包声明、其次采用 frontmatter。
没有依赖声明不影响标准格式成立，但依赖完整性为 unknown，
`--strict-metadata` 会拒绝这种不完整依赖声明。

所有来源、缓存命中、锁定与离线安装都检查标准格式。旧锁和安装记录仍可审计，
`verify` 会报告不合规入口；完整且未修改的旧包可以升级为有效版本。
格式失败不自动修补原文，也不改内容摘要。

目录发现测试不等同于真实 Agent 会话测试。客户端的刷新、信任、启用状态和
云端同步仍由客户端管理；本 CLI 不启动 Agent 或替它同步云端设置。

[返回使用指南](README.md) · [安装与事务](03-installation.md)
