# MSSQL driver

`datara-driver-mssql` implements `DatabaseDriver`/`DatabaseSession` on
Tiberius 0.13 (TDS 7.3, rustls TLS). One TCP stream per session; `cancel`
sends a TDS attention packet. `EncryptionMode` maps onto Tiberius encryption
settings; `trust_server_certificate` maps to cert validation opt-out — both
come from the `ConnectionProfile` verbatim. `quote_ident` wraps identifiers
in `[brackets]`, escaping `]` as `]]`. TDS column types convert to
`domain::Value` in `convert.rs`.

**Implemented:** trait surface it will satisfy. **Pending:** the driver
itself — phase 2 (task 2.2), proven by `SELECT 1` against a containerized
SQL Server.
