# Contributing

## Setup

```sh
mise install      # pinned Rust toolchain
make setup        # clippy/rustfmt, cargo-audit, cargo-deny, fontconfig
make build
```

`make test`, `make lint`, `make fmt-check`, and `make docs-check` must pass
before committing. See `AGENTS.md` for the full target list and the
architecture rules (no business logic in Slint, no DB calls from UI, secrets
never leave Secret Service).

## Commits

Focused commits, one concern each, conventional messages:
`feat(<crate>): ...`, `fix: ...`, `docs: ...`, `chore: ...`, `refactor: ...`.
The repo stays buildable at every commit.

## Doc-sync

Code changes that alter documented behavior update the docs **in the same
commit** — no follow-up doc debt. Adding/removing a documented file means
updating `docs/README.md`; `make docs-check` verifies the index.

## License

GPL-3.0-only (required by Slint's free tier). Contributions are licensed
under the same terms.
