# TLS

Connections to SQL Server use rustls through Tiberius (`tds73` feature).
`EncryptionMode` is a per-profile setting with three values, mapped in
`crates/driver-mssql/src/driver.rs`:

| `encryption` | Tiberius `EncryptionLevel` | Behavior |
|---|---|---|
| `disabled` | `Off` | No session encryption. Tiberius still encrypts the login packet, so the password never travels in clear text; everything after login is plaintext. Explicit user choice — for trusted local/dev instances only. |
| `preferred` | `On` | Encrypt if the server supports it; fall back to unencrypted otherwise (excluding the always-encrypted login packet). Default. |
| `required` | `Required` | Refuse the connection unless the full session is encrypted. |

`trust_server_certificate` is an orthogonal per-profile boolean:

| `trust_server_certificate` | Effect |
|---|---|
| `false` (default) | rustls validates the server certificate chain and hostname. Validation failure aborts the connection. |
| `true` | `config.trust_cert()`: accept any certificate. Only meaningful when TLS is negotiated; protects nothing — appropriate for dev containers with self-signed certs. |

Combined semantics: `disabled` + `trust` is a no-op (no TLS to verify).
`preferred` + `trust` encrypts opportunistically but never verifies.
`required` + `trust` gives encryption without authentication — still
vulnerable to MITM; use only deliberately.

**No silent downgrade of verification:** Datara never retries a failed
handshake with verification off. Certificate and handshake failures
surface as `DomainError::Tls` with the underlying rustls/Tiberius
message, so the user sees *why* the connection failed and must flip
`trust_server_certificate` themselves in the connection dialog.
