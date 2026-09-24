# Skill BOM CLI requirements

This document implements the intent of
[Issue 1](https://github.com/coolplayagent/skill-bom-cli/issues/1).
The normative scope is Skill acquisition, dependency resolution, independent
project/global installation, verification and BOM export. Runtime tool installation,
MCP configuration, registry publishing and Agent-specific adapters are excluded.
See [design](../design/skill-bom-cli.md), [AgentCenter extension](../design/agentcenter.md)
and [test evidence](../test/skill-bom-cli.md).

| ID | Requirement and acceptance condition |
| --- | --- |
| R01 | Strict versioned TOML; explicit sources; unknown fields include parser position; no credentials in locks. Project-relative paths and CLI overrides are distinct. |
| R02 | Separate skill.toml identity/dependencies. Existing entrypoints remain byte-identical. Missing metadata remains unknown. Exact-source user supplements cannot override upstream declarations. Strict mode refuses incomplete metadata. |
| R03 | Single version per source identity; Rust SemVer semantics, highest compatible versions, deterministic backtracking, lock preference, targeted updates, complete conflict chains, cycle and resource errors. Revision identities never satisfy SemVer. |
| R04 | Git HTTPS/SSH, configurable tags, subdirectories and full-commit locks. Moving tags cannot replace locked bytes. No hooks, recursive submodules, or unmaterialized LFS content. |
| R05 | SHA-256-pinned HTTPS ZIP/tar.gz with explicit package root. Reject traversal, links, devices, duplicate files, case collisions and bounded-resource violations. |
| R06 | Owner-qualified ClawHub identity, bounded complete pagination, ZIP and fixed GitHub handoff, per-file/upstream hashes, historical-version proof, rate limits, text errors, protocol errors and scan observations. No blocked-source bypass. |
| R07 | Matching locks and verified immutable cache yield identical bytes. Changed remote content and corrupt cache never silently succeed. |
| R08 | Acquire all content before deployment; target lock, same-filesystem staging, backup/journal, rollback and next-writer crash recovery. Preserve local modifications and unmanaged paths. |
| R09 | Independent OS-standard global and project scope, explicit target ownership, safe installation names, separate deployment-name conflict errors. |
| R10 | Offline prohibits HTTP/Git access. Complete cache works; absent/corrupt cache reports failure. Frozen means locked plus offline. |
| R11 | Explicit lock/installed BOM views, complete graph and evidence scopes, unknown metadata, user supplements, drift diagnostics, deterministic UTC timestamps, no secrets or absolute target paths. |
| R12 | SPDX 2.3 official Schema, unique identifiers/references, root DESCRIBES and DEPENDS_ON edges, no invented PURL, filesAnalyzed false, archive/tree checksum distinction and NOASSERTION for unknown licensing. |
| R13 | Pinned Cargo/Bazel graph and toolchains; independent unit/integration gates, >=90% Rust line coverage, Miri/ASan, architecture and file budgets, Linux/macOS/Windows CI and truthful final Qualitygate evidence. |
| R14 | Archive, Git and ClawHub source adapters and source orchestration are separate Bazel `rust_library` compilation units with Cargo API parity; a version-matched CLI Skill plus platform binaries forms one verified release archive, and a navigable book documents use, formats, architecture and verification. |
| R15 | An explicit AgentCenter Registry uses stable skillId identity, origin-bound X-Auth-Token, direct detail and versioned ZIP download, validated response and archive bytes, independent archive/tree hashes, existing cache and install transactions, and a separate Bazel library. Fresh resolution offers only the reported latest SemVer; unavailable historical versions fail closed. |

Required CLI commands are init, validate, lock, update [alias], install, tree,
why, list, verify and bom. Install supports locked/frozen/dry-run; common options
select manifest, global scope, target, offline access, JSON output and strict
metadata. `schema` additionally exports implementation-generated JSON Schemas.

Exit 1 denotes a definite conflict, content mismatch or drift; exit 2 denotes
invalid input or an incomplete I/O/resource operation. Interruptions use 130.
Successful output may include explicit non-blocking unknown/suspicious warnings.
Acceptance requires exercised evidence, not merely presence of an implementation
or test definition. Live service and non-host platform runs remain separately
identified until actually executed.
