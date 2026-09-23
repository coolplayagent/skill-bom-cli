# Skill BOM CLI

`skill-bom` resolves Skill dependencies, pins immutable content, installs it into
an ordinary directory, and exports auditable JSON or SPDX 2.3 bills of materials.
It never executes Skill content, installs runtime tools, configures MCP, or
publishes to a registry. `skill.toml` is this project's dependency protocol,
not a ClawHub standard.

Requires Git for Git sources. Rust 1.97.1, Bazel 9.2.0 and rules_rust 0.74.0 are
pinned. Cargo.lock owns the dependency graph used by both build entry points.

```sh
cargo build --locked
bazel build --lockfile_mode=error //:skill-bom
skill-bom init
# Edit skills.toml; see examples/skills.toml.
skill-bom validate
skill-bom lock
skill-bom install --locked
skill-bom tree
skill-bom why review
skill-bom verify
skill-bom bom --from lock --format json
skill-bom bom --from installed --format spdx-json
```

Commit `skills.toml` and `skills.lock`. Ordinary installation retains the lock;
a changed declaration requires `lock`. Use `update` or `update <root-alias>` to
request upgrades. `install --frozen` requires a matching lock and verified cache,
and makes no network requests. `install --dry-run` resolves and checks content
and reports additions, replacements, removals and conflicts without creating or
modifying the installation target or lockfile. It may populate the content cache.

The default project target is `skills/` beside the manifest. `--manifest PATH`
selects another project. `[install].target` is relative to the manifest; the
`--target PATH` override is relative to the invocation directory. `--global`
uses standard OS configuration, data and cache directories, independently of
project environments. Actual manifest, lock and target paths are printed to
stderr. JSON command results go to stdout; errors/diagnostics go to stderr.

ClawHub references must be owner qualified (`@owner/slug`). HTTPS archives need
an exact `=x.y.z` version and SHA-256. Git tags default to `v{version}`; a `rev`
selector instead pins a full commit without inventing a semantic version.
Credential-bearing URLs and URL query strings are rejected. SSH uses the
conventional `git` user and existing Git credentials. A registry's `token_env`
selects its Bearer token; the value is never serialized or sent across origins.
Loopback HTTP is allowed for local tests; remote archives/registries require HTTPS.

A Skill must contain exactly one of `SKILL.md`, `skill.md`, or `skills.md`.
Structured metadata fixes its name and dependencies. Legacy Skills use the
frontmatter name and have **unknown dependencies**, unless an exact-source
`[[package_metadata]]` supplement is provided. `--strict-metadata` requires
upstream declarations or a `complete = true` supplement. Supplement examples
are in [the design](codespec/design/skill-bom-cli.md).

Targets have one owner. Unmanaged directories are never overwritten and modified
managed packages cannot be replaced or removed. Installation uses an exclusive
lock, same-filesystem staging and a recovery journal. A later writer rolls back
an interrupted transaction. Several package directories are not simultaneously
atomic to external readers; avoid Agent reads during deployment. Shared cache
entries are reverified on every use and are never automatically garbage collected.

BOM lock view describes expected content, not deployed content. Installed view
verifies the installed record and still emits a diagnostic BOM on drift (exit 1).
Recorded scan observations are historical; BOM export performs no network calls.
Set `--timestamp 2026-01-01T00:00:00Z` or `SOURCE_DATE_EPOCH` for stable output.
Unknown licenses become `NOASSERTION`; no `pkg:skill` PURL is invented.

Exit codes: 0 success, 1 deterministic failure/drift, 2 invalid input or incomplete
operation, 130 interruption. JSON errors carry `code`, `phase`, `package`,
`chains`, `hint`, and `exit_code`. See [error codes](codespec/design/errors.md).

Development and acceptance evidence are specified in
[requirements](codespec/requirements/skill-bom-cli.md),
[design](codespec/design/skill-bom-cli.md), and
[test plan](codespec/test/skill-bom-cli.md). Do not infer verification status from
this README; see the generated final verification report for the tested snapshot.

For isolated automation, `SKILL_BOM_HOME` may point to an absolute directory;
its `config/`, `data/` and `cache/` replace the standard user directories. Tests
use this override to avoid modifying real user environments on every platform.
