# AgentCenter Registry adapter (Issue #2)

This extension implements the [Issue #2](https://github.com/coolplayagent/skill-bom-cli/issues/2)
request using the behavior reported there. The RelayAgent source, a formal
AgentCenter API Schema, credentials and a live service were unavailable in this
repository. Local contract tests exercise the reported shapes; they do not
establish that the internal service currently serves those shapes. The
[test contract](../test/skill-bom-cli.md) keeps that gap explicit.

## Declaration and identity

`[registries.alias]` accepts `kind = "agentcenter"`, a credential-free HTTPS
base URL and required `token_env`. The named environment variable supplies the
W3 `X-Auth-Token` value; it is never serialized. Each dependency uses the
Registry alias, a stable `skillId` in `package`, and a SemVer `version` request.
`tag` and `rev` are unsupported. An optional explicit `subdir` selects the ZIP
package root; without it, the archive root must contain exactly one recognized
Skill entrypoint. No recursive guess is made. The `PackageId` is the Registry
URL, skillId and subdirectory, independently of display name or alias.

```toml
[registries.market]
kind = "agentcenter"
url = "https://agent.huawei.com"
token_env = "AGENTCENTER_W3_TOKEN"

[dependencies.review]
registry = "market"
package = "review-skill-id"
version = "=1.2.3"
# subdir = "package/root"  # only when the ZIP has an explicit wrapper
```

The Registry detail endpoint returns only `latestVersion` in the reported
contract. Consequently, a fresh resolution exposes **only that one SemVer
candidate**, even for a range. If it does not satisfy the request, resolution
fails; the adapter does not invent a version catalog or silently install a
different release. Fresh locking of a historical release is rejected until a
version-specific detail/inventory contract is available. An existing lock can
re-fetch its older version using the download API and accepts it only when the
archive SHA-256 still matches the lock. Existing verified cache works offline.

## HTTP and content contract

The adapter calls `GET /mcpService/external/skills/v1/get?skillId=...` and
`POST /mcpService/external/skills/v1/download` with JSON `{skillId,version}`.
The detail response must contain a successful `code` (0, 200 or 20000) or
`success: true`, an object `data`, the exact requested `skillId` and a valid
SemVer `latestVersion` (or `version`). Numeric/string business codes are
accepted. HTTP 401/403 and business 40100/40300 fail as `AUTH_REQUIRED`;
there is no fallback to Git. Non-JSON errors are reported by HTTP status,
without echoing response bodies. Requests have bounded retries, timeouts and
response sizes. AgentCenter authenticated requests reject redirects; TLS
verification remains enabled for non-loopback HTTPS. Credentials are not sent
to any other origin.

The download must be ZIP bytes with ZIP or octet-stream content type. Shared
archive extraction rejects traversal, links, duplicate paths, case collisions
and resource-limit breaches. The content-addressed store records the archive
SHA-256 and the independent `skill-tree-sha256-v1` inventory, rechecking both
on cache repair and deployment. `skill.toml`, if present, must agree with the
selected version and can declare transitive Skill dependencies. Without it,
dependency metadata stays unknown unless the root supplies an exact supplement;
`--strict-metadata` applies unchanged. No upstream signature or per-file digest
is claimed because the reported AgentCenter contract does not supply one.

Unlike RelayAgent's reported list/search flow, this CLI requires an explicit
skillId and uses direct detail lookup. It does not need the name-deduplicating
`POST .../skills/v2/query` endpoint, whose list does not provide a complete
version catalog. This avoids merging distinct skillIds and keeps package
identity stable. The issue's reported default-disabled TLS and heuristic
`SKILL.md` root discovery are deliberately not adopted.

The adapter is a separate `//src/sources:agentcenter` `rust_library` alongside
other sources. `net` owns GET/POST transport and auth-header confinement;
`sources/agentcenter.rs` owns protocol, identity and ZIP acquisition;
`store` and `installer` retain content verification and transaction ownership.
This is a Registry source, not an Agent-specific installation adapter. The
[requirements](../requirements/skill-bom-cli.md) exclude Agent search-path logic.
