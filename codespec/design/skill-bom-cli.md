# Skill BOM CLI design

See [requirements R01–R14](../requirements/skill-bom-cli.md) and
[test contract](../test/skill-bom-cli.md). Rust library modules are reusable;
the binary installs interruption handling and delegates parsing/orchestration.

## Declarations and identity

`skills.toml` holds project, target, root registry aliases and dependencies.
`skill.toml` holds publisher identity, version, optional description/license and
dependencies using the same source model. TOML rejects unknown fields. Remote
packages can only reference aliases already configured at the root.

Sources are identity-bearing enums: normalized Git repository plus subdirectory,
stable archive URL, or registry URL plus lowercase owner and slug. Aliases do not
rename packages. Package directory names are portable, case-collision checked,
and cannot reserve the `.skill-bom` control namespace.

An archive requires `version = "=1.0.0"`, SHA-256 and optional `subdir`. A Git
dependency requires either a SemVer range (default tags `v{version}`) or `rev`.
ClawHub requires `package = "@owner/slug"` and either version or tag. URLs reject
credentials/query/fragment; use SSH credentials or registry token_env instead.
Registry token values are never serialized. Public requests are anonymous.

Legacy frontmatter name/description are auxiliary metadata; body text is never
interpreted as commands or dependencies. Frontmatter version discrepancies are
diagnostics. A structured version discrepancy is an error. A revision snapshot
retains its commit identity even if skill.toml has a version.

Exact supplements are arrays with a nested source and dependency table:

```toml
[[package_metadata]]
name = "legacy-review"
complete = true
[package_metadata.source]
git = "https://github.com/example/skills.git"
subdir = "review"
rev = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
[package_metadata.dependencies.context]
registry = "clawhub"
package = "@example/context"
version = "^1"
```

Supplements are matched by canonical PackageId and exact version/full commit.
Their canonical JSON digest and origin appear in locked metadata. Incomplete
supplements stay incomplete; upstream skill.toml takes precedence by rejecting
any matching override instead of silently merging it.

## Resolution and immutable sources

The resolver uses explicit heap backtracking frames, deterministic PackageId
ordering and highest SemVer candidates. It tries compatible old lock candidates
before making a catalog request. Updating one alias removes that root's lock
preference, retaining other nodes unless constraints force changes. Graph paths
explain conflicts and cycles. Limits: 256 packages, 2,048 source candidates,
10,000 expanded paths/backtracking steps, 64 registry pages. Exhaustion is an
error, never an empty candidate set or success.

Git uses argv-based processes in disposable bare repositories, enumerates tags,
fetches exact commits and archives only the selected tree. It never checks out
or runs scripts/hooks, and detects gitlinks and LFS pointers. Process output is
bounded to 128 MiB (stderr 64 KiB), with a 90-second operation deadline.

HTTP is sequential (concurrency 1), has 10-second connection/30-second request
timeouts, at most 5 redirects and 2 retries, and a 90-second retry budget.
Retry-After delay seconds and HTTP dates are respected within that budget.
Authorization is attached only to the original registry origin, never a GitHub
handoff or cross-origin redirect. Error bodies may contain credentials; errors
report status and operation without echoing remote bodies.

ClawHub contract baseline is
`826992bd72b9f9ab09254dc43551facdf94cb07b`, using ownerHandle on all skill,
version and download queries. Canonical owner/slug are verified before reads.
Versions are paginated with duplicate cursor rejection. ZIPs verify the exact
release and any supplied file inventory. The API's current `public-github`
descriptor cannot bind a historical SemVer release; those requests fail with
SOURCE_VERSION_UNAVAILABLE. A latest snapshot locks its full commit and verifies
repo/path/contentHash/commit-bound GitHub archiveUrl. Its folder hash uses the
upstream path/NUL/size/NUL/SHA-256/newline encoding with English ICU collation;
it is distinct from the native tree digest. No alternate-source fallback bypasses
registry block decisions. Scan observations include UTC observation time.

Archives are limited to 64 MiB compressed, 128 MiB expanded, 4,096 files and
8,192 total entries. ZIP and tar extraction reject traversal, absolute paths,
backslashes, reserved Windows names, links, special files, repeated files and
case-folded conflicts. No heuristic top-level-directory stripping is used for
ordinary archives. GitHub ZIP has exactly one repository root, then explicit
handoff path selection. Files are copied; no installation symlinks are created.

## Lock, cache and installation facts

Lock schema 1/solver semantics 1 stores manifest digest, root/child aliases and
requests, source identity, exact version/revision, acquisition declaration,
content inventory, directory, metadata origins and evidence. JSON BTreeMaps
provide deterministic ordering. No timestamps are introduced except genuine
ClawHub scan observations; a retained lock retains those observations.

`skill-tree-sha256-v1` hashes ASCII `skill-tree-sha256-v1` followed by NUL,
then files sorted by normalized relative UTF-8 path. Each file contributes
unsigned 64-bit big-endian path byte length, path bytes, unsigned 64-bit big-endian
file length and 32 raw SHA-256 bytes. Timestamps/compression and permissions are
excluded. Executable permission is separately recorded and checked on Unix;
Windows does not claim Unix executable-bit verification. Empty directories are
not package content. Cache reads recompute inventory and tree checksums. Online
corruption is replaced with reverified content; offline corruption fails.

The target's `.skill-bom` directory owns owner.json, write.lock, state.json and
transaction.json. Owner identity hashes the normalized manifest path; local
paths never enter the portable lock or BOM. Installed state stores its exact lock
and digest, independently of a subsequently edited project lock.

All downloads and content verification precede target acquisition. Under a
nonblocking exclusive filesystem lock, stage packages on the same filesystem,
check local drift/unmanaged conflicts, persist a journal, rename old packages to
backups, rename staged directories into place, write installation state and mark
the journal committed. Each state publication uses atomic file replacement and
Unix directory synchronization. An error rolls back. A crash leaves a durable
journal; the next writer restores backups and the old record, or cleans a committed
transaction. Read-only views report pending recovery. Deletions only remove
unchanged packages listed in the old record. This is recoverable multi-package
deployment, not simultaneous atomic visibility to external Agents.

## BOM and schemas

Lock view exports intended resolved content without claiming installation.
Installed view exports the deployed record, content status, unmanaged names and
differences from the current lock. Drift still emits BOM with exit 1. Export does
not refresh scan status. Timestamps are UTC; explicit timestamp or SOURCE_DATE_EPOCH
makes exports repeatable. Schemas are generated using `skill-bom schema KIND`.

SPDX 2.3 maps project/skills to Packages, DESCRIBES/DEPENDS_ON relationships,
versionInfo and actual archive SHA256 checksums. Custom tree and upstream evidence
live in JSON package comments, with scope/algorithm labels. Arbitrary declared
license text is retained in comments; recognized SPDX IDs are mapped, and unknown
values are NOASSERTION. Files are not individually license analyzed. Official
Schema is pinned to spdx/spdx-spec v2.3; it is complemented by reference validation.

## Module ownership

`domain` contains data and pure invariants; `resolver` uses SourceProvider's
standard candidate/metadata boundary. `config`, `sources`, `store`, `installer`,
`bom` and `application` own the corresponding workflows. `interfaces` owns
Clap/output rendering. `env`, `paths`, `net`, `process` centralize environment,
path confinement, HTTP and subprocess APIs. The Rust quality test checks the
module import graph, domain I/O prohibition and reserved boundary imports.

Cargo composes these modules through `src/lib.rs`. Bazel compiles shared
contracts/I/O boundaries in `//:foundation`, archive, Git and ClawHub adapters
separately in `//src/sources:archive`, `:git` and `:clawhub`, their provider in `:sources`, then links
`//:skill_bom`. Bazel-only entry points re-export the same public module paths;
the source files have one implementation. The narrow `srcs` sets preserve action
cache hits for untouched adapters. `//:skill-package` packages a native CLI with
the Skill descriptor. A version tag assembles native runner binaries into one
Skill archive with a SHA-256 manifest. The [book](../../docs/README.md) covers
user workflows and contributor operations; CodeSpec retains normative contracts.
