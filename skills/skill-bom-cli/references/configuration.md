# Configuration and files

`skills.toml` declares project dependencies. `skill.toml` describes a Skill's
package identity and dependencies. The generated `skills.lock` records exact
source identities, selected versions/revisions, dependency edges and hashes.
Use `schema manifest|package|lock|installed|bom|error` for machine-readable formats.
Never put passwords or tokens in declarations, locks, BOMs or Skill content.

## Registry example

```toml
schema_version = 1

[project]
name = "example"

[install]
agent = "universal"

[registries.market]
kind = "agentcenter"
url = "https://agent.huawei.com"
# token_env = "AGENTCENTER_TOKEN"

[dependencies.review]
registry = "market"
package = "explicit-skill-id"
version = "=1.2.3"
```

Replace the example skillId/version with verified service identifiers. A ClawHub
registry uses `kind = "clawhub"`, its HTTPS URL and an owner-qualified
`package = "@owner/slug"`. Registry aliases are declared at the root. A nonempty
`token_env` overrides that origin's saved [account](authentication.md); the value
in TOML is an environment variable name, never the token.

## Other sources

Each dependency selects exactly one of `registry`, `git` or `url`:

```toml
[dependencies.git-example]
git = "https://github.com/example/skills.git"
rev = "0123456789abcdef0123456789abcdef01234567"
subdir = "git-example"

[dependencies.archive-example]
url = "https://example.org/archive-example.tar.gz"
version = "=1.0.0"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

These are illustrative identities and checksums; obtain real values before use.
Git supports HTTPS/SSH, exact commits and configured tags; its credentials remain
with Git. HTTPS ZIP/tar.gz archives require an exact version and SHA-256. Set
`subdir` explicitly when the Skill is below the archive/repository root.

Every selected package root needs an exact `SKILL.md` with valid Agent Skills
`name` and `description`. Names must match the directory; resources and client
extensions are preserved unchanged. `skill.toml` provides dependency metadata;
without it, dependency completeness remains unknown.

## Installation targets

The default universal/Codex project and user targets are `.agents/skills`.
Claude Code uses `.claude/skills`, Cursor `.cursor/skills`, and RelayAgent project
`.skills` / user `~/.relay/skills`. `--global` selects the user scope.
`--agent` and `--target` conflict at the same layer, as do `install.agent` and
`install.target`. CLI selection overrides the manifest. Keep explicit legacy
targets until the user requests a change.

`SKILL_BOM_HOME` isolates configuration, cache and user-scope Agent paths for
testing. It does not provide a plaintext alternative to the system keyring.
See [commands](commands.md) for deployment and audit workflows.
