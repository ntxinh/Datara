# Integration tests

Workspace `cargo test` only runs member-package tests, so the live SQL Server
tests live in `crates/driver-mssql/tests/` (`mssql.rs` + shared
`tests/common/mod.rs` container harness). Run them with:

    cargo test -p datara-driver-mssql

They require a rootless podman socket (`$XDG_RUNTIME_DIR/podman/podman.sock`)
and skip cleanly without one.
