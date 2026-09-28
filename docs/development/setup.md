# Development setup

Fedora 44 is the reference environment; any Linux with a C toolchain works.

```sh
mise install     # Rust 1.98.1 per mise.toml (rust-toolchain.toml pins the
                 # same channel for rustup/cargo when mise isn't on PATH)
make setup       # clippy/rustfmt components, cargo-audit, cargo-deny, fontconfig
make build && make run
```

fontconfig-devel is required for Slint's font stack (`pkg-config --exists
fontconfig`); `make setup` installs it or falls back to
`scripts/fetch-fontconfig-stub.sh` for minimal environments.

MSSQL integration tests need a container runtime — Podman, with
`DOCKER_HOST` pointing at the podman socket. See `testing.md`.
