# Testing

- Unit tests live beside code in `#[cfg(test)] mod tests`; `rstest` for
  parameterized cases.
- Integration tests live in `crates/<x>/tests/` and top-level `tests/`, so
  `cargo test --workspace` (`make test`) runs everything.
- MSSQL-backed tests use testcontainers over Podman. They detect the
  container socket (`DOCKER_HOST`) and skip with an `eprintln` when absent —
  they never fail for a missing daemon. `make test-integration` is the manual
  run where the socket is expected to be up.

Coverage today: `Value`/`DomainError`/`SecretReference` (domain), TOML
load/defaults and XDG paths (config), connection/history repo round-trips on
a tempfile pool (storage). Keep tests deterministic; no sleeps, no network in
unit tests.
