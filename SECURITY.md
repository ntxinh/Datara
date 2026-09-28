# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security reports. Use GitHub's
private vulnerability reporting:

https://github.com/ntxinh/Datara/security/advisories/new

Include a description, affected version/commit, and reproduction steps.
Reports are acknowledged as soon as possible; expect an initial response
within a few days.

## Supported versions

Datara is pre-1.0 MVP. Only the latest commit on `main` (and the newest
release, once published) receives security fixes — there are no backport
branches.

| Version | Supported |
|---|---|
| latest `main` / newest release | ✅ |
| anything older | ❌ |

## Credential handling (summary)

- Passwords live **only** in the system Secret Service (GNOME Keyring /
  Seahorse) over the session D-Bus; the SQLite database and TOML config
  store a `SecretReference`, never the secret.
- `Credentials` wraps the password in `secrecy::SecretString` with a
  redacting `Debug` impl; no secret reaches logs, `tracing` fields, or
  error messages.
- The local `app.db` is pinned to mode `0600`.
- TLS is never silently downgraded; certificate failures surface to the
  user and `trust_server_certificate` is an explicit per-profile choice.
- The MCP server is stdio-only, off by default, addresses connections by
  stored `conn_id` only (no arbitrary hosts), never returns secrets, and
  refuses non-`SELECT` statements unless `mcp.allow_writes` is set.

Details: `docs/security/secrets.md`, `docs/security/tls.md`,
`docs/mcp/security.md`.
