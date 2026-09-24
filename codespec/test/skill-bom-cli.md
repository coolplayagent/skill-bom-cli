# Verification contract and evidence

See [requirements](../requirements/skill-bom-cli.md) and
[design](../design/skill-bom-cli.md). This matrix identifies executable tests;
it does not predeclare passing results. Actual runs are recorded separately in
`verification/` and the final delivery report.

| Requirement | Rust test target and scenarios |
| --- | --- |
| R01 | config: unknown fields/locations, invalid combinations, credentials, identity, paths; cli: flags, validation and Schema exports |
| R02 | config: legacy metadata, strict mode, supplements, version mismatch, entrypoint ambiguity; cli: supplement/strict flow |
| R03 | resolver: highest/prerelease/backtracking, diamonds, lock preference, targeted update, conflict chains, cycles, bounded graph stress |
| R04 | sources: isolated Git tags/subdir/revisions/moving tags; source failure/security tests |
| R05 | sources: ZIP/tar/gzip, traversal, links, duplicates, case conflicts, size budget, corruption |
| R06 | sources: Rust loopback server and injectable HTTP contract fixtures, ownerHandle, pagination, ZIP file evidence, handoff commit/hash, historical rejection, unknown protocol, rate limits/text errors |
| R07 | sources/cli: retained lock, fixed bytes, cold/hot cache, corruption repair and mismatch rejection |
| R08 | transactions: faults before/after backup/replace/record, crash simulation and next-writer recovery, contention, unmanaged paths and tampered records |
| R09 | config/cli/transactions: global/project target independence, override and owner conflicts, directory name collision |
| R10 | sources/cli: frozen hot cache, absent/corrupt cache, network request counters |
| R11 | cli/quality: deterministic timestamp and graph, installed view with drift, unknown metadata, no absolute paths |
| R12 | cli/quality: pinned official SPDX 2.3 Schema, relationship references, checksum scope and NOASSERTION |
| R13 | quality: architecture DAG, I/O boundaries, authored file budget, docs/examples/Schemas, Cargo/Bazel toolchain alignment; independent CI gates |
| R14 | quality: source Bazel target boundaries, book reachability and links, Skill/Cargo version agreement, deterministic archive contents; Bazel builds CLI and Skill package, release workflow checks each native binary |
| R15 | agentcenter: explicit skillId/config, X-Auth-Token GET/POST, identity/business errors, latest SemVer selection, ZIP/content evidence, offline install, historical lock re-fetch and unavailable fresh historical request; Bazel adapter target and CLI lock/install/verify/BOM |

The AgentCenter fixture is derived from Issue #2's reported contract. Internal
RelayAgent source, a versioned API Schema and live service credentials were not
available here; therefore only the local HTTP fixture is verified. The API's
historical version binding remains unproven, so fresh historical resolution is
rejected. Live smoke requires an authorized service and a disposable test Skill.

Unit tests (`cargo test --locked --lib --bins --all-features`) and integration
tests (`cargo test --locked --tests --all-features`) are separate entry points.
The full policy lists integration targets explicitly to avoid counting unit tests
twice. Ordinary tests require no public network. Git repositories, HTTP servers,
caches and targets are temporary. Git identity is set per command; user Git
configuration is never changed.

ClawHub contract baseline:
`826992bd72b9f9ab09254dc43551facdf94cb07b`, specifically docs/api.md,
packages/schema/src/schemas.ts, packages/clawhub/src/cli/commands/skills.ts,
packages/clawhub/src/skills.ts and convex/lib/githubHandoff.ts. Fixture shapes
are coded in tests/sources.rs and are deliberately independent of the live
service. The transport mock supplies a fixed GitHub archive; it verifies no token
is sent to GitHub. Live smoke runs are opt-in and report availability gaps.

Bazel exposes //:skill-bom, //:skill-package, //:unit_tests, separate source
adapter libraries and each integration target; all consume
Cargo.lock. Final commands use --lockfile_mode=error. Coverage must meet 90 percent
of Rust lines. Nightly Miri runs only pure domain unit tests; ASan runs native
unit tests with a separate target directory. These are not substituted for stable
testing. Linux host evidence does not establish executed macOS/Windows evidence;
the cross-platform workflow is the execution contract for those hosts.

Resource failures and unexecuted checks are never marked passed. The final full
Qualitygate report preserves snapshot/policy digests, warnings, pending checks
and known limits. Generated lockfiles/Schemas have explicit file-budget exemptions;
authored code/tests/docs/workflows do not.
