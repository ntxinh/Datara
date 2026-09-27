# TLS

Connections to SQL Server use rustls through Tiberius. `EncryptionMode`
offers three settings per profile:

- `Disabled` — plaintext (explicit user choice, e.g. local dev containers)
- `Preferred` — encrypt if the server supports it (default)
- `Required` — refuse unencrypted connections

TLS is never silently downgraded: certificate and handshake failures surface
as `DomainError::Tls` with the underlying error. `trust_server_certificate`
is a per-profile boolean the user sets deliberately — it is not a fallback
after a validation failure.

**Implemented:** `EncryptionMode` and `trust_server_certificate` on
`ConnectionProfile`. **Pending:** driver wiring — phase 2 (task 2.2).
