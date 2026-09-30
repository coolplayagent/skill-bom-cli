# W3 authentication (Issue #3)

Implements C + B from [Issue #3](https://github.com/coolplayagent/skill-bom-cli/issues/3):
an injectable credential provider and built-in password login for skill-bom's own
one account per origin. Other applications' credentials are neither read nor modified.

## Boundaries and storage

`domain::auth` owns serializable metadata and pure expiry rules. `auth` owns
provider, secret-store and login-gateway contracts and session transactions;
`env::Clock` is injectable. `net` owns all HTTP, including the password exchange.
The resolver's candidate contract remains unchanged. Auth CLI dispatch precedes
project configuration. Status reads metadata only, even when expired.

Use `keyring-core` with official Windows Credential Manager, macOS Keychain and
Linux Secret Service backends. No plaintext fallback, external app imports or
global keyring default changes. Windows entries use local machine persistence.
An independent `org.skill-bom.w3.v1.<config-root-sha256>` service namespace isolates
config roots, including SKILL_BOM_HOME. Password and token have separate UUID-based
entries; ordinary `auth-v1/session.json` contains only account, times, expiry
source, references, login identity, version and references pending cleanup.
Native keyring errors and corrupt metadata map to AUTH_STORE_UNAVAILABLE without
backend diagnostics. Metadata writes use same-directory atomic replacement.

All mutations and token reads hold `auth-v1/session.lock` using an exclusive OS
file lock; lock acquisition is interruptible and bounded to 100 seconds. Session
versions are read again under that lock. A new credential reference is journaled
before either secret is written, then committed and the old reference deleted.
Interrupted publication remains discoverable for cleanup. Failed password
authentication leaves the prior session untouched. Logout first publishes a
versioned tombstone, then deletes password/token entries; deletion failures
retain references and return an error. Retrying logout finishes cleanup.

Refresh carries both a version and a login identity. Concurrent refreshes of the
same login reuse the committed newer token. A tombstone or different login
identity rejects the old refresh, preventing logout resurrection or account
switching during an in-flight request. An already-sent request cannot be revoked.

## Login wire contract and lifetime

The only password destination is
`https://rnd-idea-api.huawei.com/ideaclientservice/login/v4/secureLogin`.
POST JSON uses `user`, `password`, and string `requireUserInfo: "true"`, with the
Content-Type, Accept, Accept-Language and browser User-Agent specified by Issue #3.
No token accompanies login; the TLS policy below applies. Login rejects
all redirects, including same-origin and HTTP destinations. The existing bounded
network policy supplies 30-second attempts, 10-second connection timeout, two
transient retries and a 90-second retry budget. Login responses are limited to
1 MiB and must be HTTP 200. HTTP 400/401/403 reject credentials; missing/non-string/
blank `cloudDragonTokens.authToken` returns MISSING_AUTH_TOKEN. Response bodies
and keyring diagnostics are never printed. Password/token Debug is redacted.

## TLS compatibility

[Issue #4](https://github.com/coolplayagent/skill-bom-cli/issues/4) reports an
incomplete internal issuer chain. `AGENTCENTER_VERIFY_TLS` therefore defaults
to false for W3 login/refresh and AgentCenter X-Auth-Token GET/POST operations,
including custom AgentCenter registries. It disables certificate and hostname
verification, which permits interception of passwords, tokens and content even
though transport remains encrypted. This compatibility mode is for trusted
internal networks. Explicit true requires verification and never falls back.
Bearer/archive/ClawHub requests keep a separate strictly verified client;
Git transport is unaffected. Both modes preserve proxy handling, loopback proxy
bypass, fixed password destination, rejected redirects and resource limits.

Read the variable through `env` when constructing the HTTP client. Accept
case-insensitive, trimmed true/false, 1/0, yes/no and on/off; absent means false,
while empty, malformed and non-Unicode values fail with CONFIG without printing
the value. This is a process setting, not persisted auth state or a source identity.
The rustls standard roots remain in use; there is no system-store migration.

Typed rustls errors, including nested I/O causes, become NETWORK_TLS; certificate
errors have a distinct sanitized message. TLS failures are not retried and never
trigger credential refresh. Timeouts become NETWORK_TIMEOUT; other transport
failures remain NETWORK. Classification traverses at most 16 causes and never
prints raw URLs or backend messages. Login retains the transport hint alongside
the existing login guidance. Local HTTPS tests use ephemeral certificates and
exercise both accepted and rejected handshakes; no live internal login is claimed.

## Token lifetime

Positive integer seconds: `cloudDragonTokens.expiresIn`,
`cloudDragonTokens.authTokenExpiresIn`, and top-level `authTokenExpiresIn`.
RFC3339 timestamps: `cloudDragonTokens.expireTime` and top-level `expiresAt`.
Ignore values that cannot be reliably parsed or added without overflow; choose
the earliest usable value. An already-expired server timestamp rejects login.
If no usable field exists, use four hours with `expiry_source: local_policy`.
Other usable fields yield `server`. Refresh starts at half the recorded lifetime;
a backward local clock jump also requests reauthentication.

## Request policy and CLI

A nonempty configured token_env wins and is never persisted, replaced or
refreshed. Missing/empty values use the saved login only for the exact HTTPS
origin `agent.huawei.com:443`. Other origins use their own stored tokens or token_env; W3 credentials never cross into those origins. See the origin extension below.
Authenticated AgentCenter requests reject redirects. HTTP 400/401/403, empty or
JSON null responses and the existing business authentication codes permit one
refresh. Detail GET and download POST are explicitly reads and may be replayed
once; writes refresh credentials but are not replayed. Other failures are not
authentication signals. Repeated auth rejection or failed refresh supplies the
`skill-bom auth login` hint. Validation and verified-cache/offline branches do
not acquire credentials. Logout has no effect on explicit environment values.

`auth login [--username NAME] [--password-stdin]` defaults to hidden interactive
input. Noninteractive input requires both flags. The stdin reader removes only
the line terminator, preserves password spaces, requires UTF-8 and bounds the
username to 256 bytes and password to 4096 bytes. No password flag or password
environment variable exists. `auth status` reports local login, account, expiry,
expiry source, expired and pending-cleanup flags; it does not prove server validity.
`auth logout` is idempotent, with no success output on failure. All commands
support text/JSON; spdx-json remains BOM-only. See [errors](errors.md) and
[verification evidence](../test/skill-bom-cli.md).


## Origin-bound account extension (0.0.7)

Auth commands accept --origin, defaulting to https://agent.huawei.com. Origins
are canonical scheme/host/effective-port identities, without URL credentials,
paths, queries or fragments; HTTPS is required except loopback HTTP fixtures.
ClawHub base paths use the registry origin for lookup. Each origin has one
account; replacing or deleting it leaves other origins unchanged.

Official AgentCenter retains auth-v1/session.json, session.lock and the existing
W3 keyring namespace. Its serialized fields remain readable without migration.
Other origins use auth-v1/origins/<sha256-origin>/ with independent locks, revisions,
login IDs, cleanup journals and a config-root/origin-scoped keyring namespace.
Their metadata binds the exact origin and method: token. Expiry fields are null.
Catalog reads examine at most 1024 origin entries and bound each record to 32 KiB;
corrupt or misbound metadata fails closed. Status creates no files or locks.

Token login uses hidden input or --username plus --token-stdin; it never constructs
a login gateway, validates remotely or claims known expiry. The account name is
user-supplied metadata. Printable nonblank ASCII tokens are bounded to 16384 bytes.
Input flags must match the origin's method. Token storage works offline; W3 does
not. Secrets use the same journaled publication and keyring-only policy. Token
cleanup deletes only token entries, including interrupted publications.

Credential stamps include origin and method alongside version/login identity.
Only W3 credentials can refresh. Saved tokens fail on authentication rejection
with an origin-specific login hint and are never replayed or replaced implicitly.
Nonempty token_env still wins without reading metadata or the keyring. Both
AgentCenter and ClawHub acquire through the provider; absent ClawHub credentials
permit anonymous access. ClawHub's GitHub handoff still carries no registry token.
HTTP 401 on the original Bearer request is an authentication signal; existing
blocked-source and redirect policies remain in force.

Status JSON adds origin and method (w3/token), retaining the W3 fields and values.
Token expiry, expiry source and expired are null. --all returns a sorted sessions
array of saved logins and pending cleanup records; it conflicts with --origin.
Logout defaults to official AgentCenter, never all origins. A missing local login
is not remote evidence, and logged_in means only a local session is recorded.
