# Troubleshooting and evidence limits

Success JSON goes to stdout; diagnostics and structured errors go to stderr.
Exit codes: `0` completed, `1` definite conflict or drift, `2` invalid input or
incomplete I/O/resource operation, `130` interrupted. Preserve the error's code,
phase and hint; never include secrets or service response bodies in reports.

| Error | Action |
| --- | --- |
| `AUTH_INPUT` | Match username and stdin flags to the origin's login method; check input limits |
| `AUTH_REQUIRED` | Reauthenticate the stated origin, or replace its explicit token_env value |
| `MISSING_AUTH_TOKEN` | W3 returned no usable token, or the supplied provider has another origin |
| `AUTH_STORE_UNAVAILABLE` | Unlock/restore the system keyring; check local metadata/lock availability; retry pending logout |
| `NETWORK_TLS` | Check certificates, hostnames, proxy and the documented AgentCenter TLS setting |
| `NETWORK_TIMEOUT` / `NETWORK` | Check service reachability and proxy; retain the bounded failure |
| `SOURCE_BLOCKED` | Check upstream access/moderation; do not bypass blocked-source policy |
| `SOURCE_VERSION_UNAVAILABLE` | Check exact identity/version and whether the upstream can prove that release |
| `CHECKSUM_MISMATCH` | Preserve evidence; do not replace expected hashes with unverified downloads |
| `OFFLINE_MISS` | Use online acquisition to populate a verified cache, or provide the missing content |
| `SKILL_FORMAT` | Fix upstream Skill metadata; do not silently rewrite acquired bytes |
| `RESOURCE_LIMIT` | Reduce the declared input or review the reported bounded operation |

For [authentication](authentication.md), status is local evidence only. Token
storage never proves remote validity. Failed W3 login preserves the previous
account. Rejected Token sessions do not invoke W3 or replay requests. A failed
logout reports failure until its pending cleanup succeeds.

For [installation](commands.md), use dry-run and inspect target conflicts. Do not
overwrite user-modified or unmanaged files. If sync deployed successfully but
failed to write its lock, follow the reconciliation command in the error.
`install --frozen` needs both a matching lock and verified cached content.

AgentCenter fresh resolution currently offers only its reported latest SemVer;
an unprovable historical version fails. ClawHub's GitHub handoff identifies a
fixed commit, but cannot prove a historical published release. Missing dependency
metadata remains unknown in the BOM. Lock-view BOM does not prove deployment.

The CLI acquires and verifies Skill content as data. It does not execute Skill
instructions, install runtime tools, configure MCP or configure Agent runtimes.
Report tested platforms/services precisely; local fixtures do not prove live
service authentication or a real Agent runtime's behavior.
