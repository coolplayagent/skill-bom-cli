# Error contract

Errors have stable `code`, `message`, `phase`, optional `package`, root-to-node
`chains`, an actionable `hint` and numeric `exit_code`. Messages are explanatory;
clients should match codes. See [design](skill-bom-cli.md).

| Group | Codes |
| --- | --- |
| Configuration | INPUT, CONFIG, SOURCE_COMBINATION, VERSION, ARCHIVE_CONFIG, TAG_PATTERN, REVISION, REGISTRY, PACKAGE_ID, URL, SCOPE, FORMAT, ALIAS, TIMESTAMP, SCHEMA_VERSION |
| Resolution | VERSION_CONFLICT, DEPENDENCY_CYCLE, DIRECTORY_CONFLICT, RESOURCE_LIMIT |
| Metadata | METADATA, METADATA_OVERRIDE, METADATA_UNKNOWN, VERSION_MISMATCH, SKILL_ENTRYPOINT |
| Sources | SOURCE, SOURCE_IDENTITY_CHANGED, SOURCE_VERSION_UNAVAILABLE, SOURCE_BLOCKED, PROTOCOL, PAGINATION_LOOP, CHECKSUM_MISMATCH, CONTENT_CHANGED, SUBMODULE_UNMATERIALIZED, LFS_UNMATERIALIZED |
| Transport/authentication | NETWORK, NETWORK_TIMEOUT, HTTP_RETRY_EXHAUSTED, RATE_LIMIT, HTTP_STATUS, MISSING_AUTH_TOKEN, AUTH_REQUIRED, GIT_UNAVAILABLE, GIT_FAILED, GIT_TIMEOUT, GIT_PROTOCOL, REVISION_AMBIGUOUS, PROCESS |
| Files/content | IO, JSON, PATH, UNSAFE_PATH, CASE_COLLISION, ARCHIVE, ARCHIVE_LINK, ARCHIVE_DUPLICATE, ARCHIVE_SIZE, CONTENT_MISSING, CACHE_KEY, CACHE_UNAVAILABLE, CACHE_CORRUPT, OFFLINE_MISS |
| Installation | LOCK_REQUIRED, LOCK_INVALID, MANIFEST_CHANGED, MANIFEST_EXISTS, TARGET_OWNERSHIP, TARGET_BUSY, UNMANAGED_CONTROL, INSTALL_RECORD, INSTALL_CONFLICT, TRANSACTION_INVALID, RECOVERY_REQUIRED, NOT_INSTALLED |
| Export/runtime | PACKAGE_QUERY, SPDX, USER_DIRECTORY, SIGNAL, INTERRUPTED |

Preserve `.skill-bom` if recovery is required. Restore or save local modifications
before rerunning install. Run lock after intentional manifest edits. Retry online
to repair a missing/corrupt cache. Registry ownership changes require explicit
source declaration updates. Unsupported historical snapshots cannot be repaired
by silently fetching latest content.
