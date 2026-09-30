# Commands and workflows

Run the matching bundled executable described in [SKILL.md](../SKILL.md).
Use `--help` on the executable or a subcommand for accepted arguments.

| Command | Purpose |
| --- | --- |
| `init` | Create a minimal skills.toml without overwriting a file |
| `validate` | Check declarations; no credential lookup or service validation |
| `lock` | Resolve all dependencies and write skills.lock, without deployment |
| `update [alias]` | Upgrade all roots or one declared root alias |
| `sync [alias] [--dry-run]` | Upgrade, verify and deploy; write the lock after deployment |
| `install [--locked] [--frozen] [--dry-run]` | Install verified content, retaining locked versions |
| `tree` / `why <package>` | Explain the dependency graph and introduction paths |
| `list` / `verify` | Inspect installation records and detect content drift |
| `bom --from lock\|installed` | Export audit data from the selected evidence scope |
| `schema manifest\|package\|lock\|bom\|installed\|error` | Export a versioned JSON Schema |
| `auth login/status/logout` | Manage local [origin accounts](authentication.md) |

Global flags include `--manifest PATH`, `--global`, `--target PATH`,
`--agent universal|codex|claude-code|cursor|relayagent`, `--offline`,
`--strict-metadata`, and `--format text|json`. `--format spdx-json` is BOM-only.

## First installation

Read or create the [declaration](configuration.md), then run:

```sh
skill-bom validate
skill-bom lock
skill-bom install --locked --dry-run
skill-bom install --locked
skill-bom verify
```

Review the printed manifest, lock and target paths. Preserve the selected Agent
or target for subsequent list, verify and installed BOM commands. `--locked`
requires a matching lock. `--frozen` additionally prohibits network access and
requires complete verified cached content.

## Upgrade and audit

Use `sync [alias] --dry-run` to preview an explicitly requested upgrade, followed
by `sync [alias]`. Preview may populate cache but changes neither lock nor target.
Sync queries candidates online; it cannot run offline. A failure writing the lock
after successful deployment provides a reconciliation command.

```sh
skill-bom bom --from lock --format json
skill-bom bom --from installed --format spdx-json
```

Lock BOM proves resolution only. Installed BOM checks deployed content. Use
`--timestamp RFC3339` or `SOURCE_DATE_EPOCH` for reproducible BOM timestamps.
Missing upstream dependency metadata remains unknown; use `--strict-metadata`
when incomplete metadata is unacceptable. See [troubleshooting](troubleshooting.md).
