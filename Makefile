.PHONY: setup build run test test-integration lint fmt fmt-check check audit deny clean docs-check

# Toolchain + dev tools (cargo-audit, cargo-deny) + fontconfig headers for slint.
setup:
	mise install
	rustup component add clippy rustfmt
	cargo install cargo-audit cargo-deny --locked
	pkg-config --exists fontconfig || sudo -n dnf install -y fontconfig-devel || sh scripts/fetch-fontconfig-stub.sh

build:
	cargo build --workspace

run:
	cargo run -p datara

test:
	cargo test --workspace

# Integration tests require the podman socket (DOCKER_HOST).
test-integration:
	cargo test --workspace --test '*'

lint:
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

check:
	cargo check --workspace

audit:
	cargo audit

deny:
	cargo deny check

clean:
	cargo clean

# TODO(Task 1.7): verify every file listed in docs/README.md exists.
docs-check:
	@echo "TODO: docs index check lands with the docs tree (Task 1.7)"
