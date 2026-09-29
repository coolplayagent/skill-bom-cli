# W3 authentication (Issue #3)

Implements C + B from [Issue #3](https://github.com/coolplayagent/skill-bom-cli/issues/3):
an injectable credential provider and built-in password login for skill-bom's own
single account. Other applications' credentials are neither read nor modified.

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
No token accompanies login; TLS certificate validation stays on. Login rejects
all redirects, including same-origin and HTTP destinations. The existing bounded
network policy supplies 30-second attempts, 10-second connection timeout, two
transient retries and a 90-second retry budget. Login responses are limited to
1 MiB and must be HTTP 200. HTTP 400/401/403 reject credentials; missing/non-string/
blank `cloudDragonTokens.authToken` returns MISSING_AUTH_TOKEN. Response bodies
and keyring diagnostics are never printed. Password/token Debug is redacted.

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
origin `agent.huawei.com:443`. Custom registries require an explicit token.
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
