# Agent Skills compatibility (R19–R20)

The normative format source is https://agentskills.io/specification and its
official skills-ref name validation implementation. The compatibility contract
validates the six standard fields while preserving additional client fields.
Only exact SKILL.md is an entrypoint. Lowercase legacy entrypoints are never
renamed. Original content bytes, resources and content hashes are preserved.

`domain::skill` owns serializable Agent/SkillFrontmatter contracts, Unicode/NFKC
name invariants and portable directory collision keys. `config::skill` parses
UTF-8 and line-delimited YAML with serde-saphyr's deserialize-only feature.
An intermediate typed JSON value prevents implicit number/bool-to-string
coercion. Optional fields, when present, cannot be null. Unknown client fields
are data and do not grant execution privileges.

Limits: SKILL.md 1 MiB, frontmatter 64 KiB, nesting/flow/replay depth 32,
10,000 parser/replayed/retained events, 5,000 nodes, 64 anchors/aliases and
64 expansions per anchor. Scalar/retained payload budgets are 64 KiB.
Duplicate keys and unsupported tags fail. External inclusion and property
interpolation features are disabled. YAML syntax errors retain parser locations.
The Markdown body remains opaque; authoring recommendations are not hard limits.

Names are trimmed and NFKC-normalized, then checked for 1–64 Unicode lowercase
alphanumeric characters and single interior hyphens. Windows reserved names
remain forbidden. Deployment uses the canonical name, not a temporary/cache
directory basename. Generic manifest aliases keep their existing name rules.
The portable lock reader accepts safe historical names; new content is checked
against both locked directory and metadata name before any deployment.

`sources` always reads standard metadata, even with skill.toml or supplements.
Description comes from SKILL.md. License comes from skill.toml when present,
otherwise SKILL.md. Dependency metadata origin, version checks and
--strict-metadata retain their separate meaning. Missing skill.toml implies
unknown dependency completeness, not an invalid standard Skill.

`Provider::ensure` validates after verified cache reads, outside the cache-repair
fallback: invalid frontmatter cannot be repaired by retrying the network.
`installer` also validates all desired packages before staging, including
unchanged packages. Public verification adds standard/Agent diagnostics to the
integrity result; transactional preflight uses content integrity so unchanged
historical packages can be replaced or removed. Historical records are never
rewritten merely by reading them.

The optional `[install].agent` uses kebab-case enum strings. None is omitted
from serialization to preserve the old manifest digest. At each layer, agent
and target are mutually exclusive; a CLI selector replaces either manifest
selector. Absent selection defaults to universal. `init` persists explicit
selection. No machine-specific target is stored in portable locks or BOMs.

| Agent | Project, relative to manifest | User home |
| --- | --- | --- |
| universal / codex | .agents/skills | .agents/skills |
| claude-code | .claude/skills | .claude/skills |
| cursor | .cursor/skills | .cursor/skills |
| relayagent | .skills | .relay/skills |

`env` owns user-home resolution. SKILL_BOM_HOME supplies an isolated home/
alongside existing config/data/cache roots. Explicit targets retain old path
semantics. Old default targets are only inspected to report an existing owned
installation; no automatic migration or deletion occurs. One target retains
one owner, and each Agent deployment is a separate transaction.

Claude Code reserves synced and anthropic-skills. Other runtime enablement,
trust and cloud synchronization belong to the Agent. Official path references
and user workflows are in the [book](../../docs/02-user-guide/05-agent-skills.md).
Tests implement filesystem discovery contracts only and use isolated homes.
