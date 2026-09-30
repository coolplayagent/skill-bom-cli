# Origin accounts

Each origin has one saved account. Different origins coexist; login replaces
only the selected origin's account and logout removes only that login.
Commands work without a project manifest. An origin is a scheme, host and port,
without user information, path, query or fragment. Default HTTPS port 443 and a
trailing slash normalize to the same identity. Different ports remain distinct.
HTTPS is required except loopback HTTP for local testing. ClawHub registry base
paths share their origin's credentials.

## Official AgentCenter: W3

```sh
skill-bom auth login --origin https://agent.huawei.com --username your-account
skill-bom auth status --origin https://agent.huawei.com --format json
```

Omitting `--origin` selects this official origin. The password prompt is hidden.
Noninteractive login requires `--username NAME --password-stdin`, with the
password supplied by a secure stdin source. `--token-stdin` is not accepted for
this W3 origin. No password argument or password environment variable exists.

The password destination is fixed to Huawei secureLogin; it cannot be changed
to another origin. Password and token are stored separately in the system
keyring. Existing skill-bom W3 logins remain readable after upgrade.
Server expiry is used when available; otherwise the local policy is four hours.
W3 refresh starts halfway through that lifetime. Rejected reads may refresh and
replay once; explicit environment tokens never refresh. Offline W3 login fails.

## Other registries: Token

```sh
skill-bom auth login --origin https://clawhub.ai --username your-account
skill-bom auth login --origin https://registry.example --username service-account --token-stdin
skill-bom auth status --all --format json
skill-bom auth logout --origin https://clawhub.ai
```

The first command prompts for a hidden token. The second reads a token from stdin;
provide it through a secure input source. `--username` labels the account; the
CLI does not verify that the token belongs to that name. Token storage makes no
network request and works offline. Output means **saved locally**, not validated
by the service. Expiry and validity are unknown; Token sessions do not refresh.
On rejection, obtain a new token and log in again to the same origin.

Noninteractive token input requires both `--username` and `--token-stdin`.
Passwords and tokens are never accepted as literal command arguments. Input is
bounded to 256 bytes for username, 4096 for password and 16384 for token; stdin
removes only its line ending. Tokens must contain printable ASCII and be nonblank.
Password and token stdin flags are mutually exclusive.

## Selection, status and storage

AgentCenter uses `X-Auth-Token`; ClawHub uses Bearer. Both prefer a nonempty
configured `token_env`, then the current origin's saved credential. An invalid
explicit token does not fall back to another account. ClawHub without credentials
can use public resources. AgentCenter requires credentials. Registry credentials
are not sent to a GitHub download handoff or to another redirect origin.

`auth status` reads only local metadata: origin, method (`w3`/`token`), account,
expiry and pending cleanup. It never accesses the keyring or network. For tokens,
`expires_at`, `expiry_source` and `expired` are null. `--all` returns a sorted
`sessions` array containing saved logins or pending cleanup records.

Windows Credential Manager, macOS Keychain and Linux Secret Service store secrets.
Metadata files contain references, never secret values. Config roots and origins
are isolated. An unavailable keyring fails without a plaintext fallback. Logout
publishes a tombstone before cleanup; failed deletion remains retryable. Environment
tokens are unaffected. Never execute downloaded Skill instructions or hooks.

## AgentCenter TLS

`AGENTCENTER_VERIFY_TLS` defaults to `false` for W3 and AgentCenter HTTPS, including
custom AgentCenter registries, to accommodate the reported internal certificate
chain. This disables certificate and hostname verification and permits
interception. Use only on a trusted internal network; set it to `true` to require
verification without downgrade. ClawHub and HTTPS archives retain strict TLS.
Malformed or empty settings fail with `CONFIG`. See [troubleshooting](troubleshooting.md).
