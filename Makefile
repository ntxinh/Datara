.PHONY: setup build run test test-integration lint fmt fmt-check check audit deny clean docs-check rpm
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

# Build the source tarball (git archive HEAD) and run rpmbuild locally.
# Requires rpm-build, cargo/rust >= 1.98, fontconfig-devel, freetype-devel.
# Artifacts land in packaging/rpm/_build/{SRPMS,RPMS}.
rpm:
	set -euo pipefail; \
	VERSION=$$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1); \
	TOPDIR="$(CURDIR)/packaging/rpm/_build"; \
	mkdir -p "$$TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}; \
	git archive --format=tar.gz --prefix="datara-$$VERSION/" -o "$$TOPDIR/SOURCES/datara-$$VERSION.tar.gz" HEAD; \
	cp packaging/rpm/datara.spec "$$TOPDIR/SPECS/"; \
	rpmbuild -ba --define "_topdir $$TOPDIR" "$$TOPDIR/SPECS/datara.spec"

# Verify every file linked from docs/README.md exists.
docs-check:
	@missing=0; \
	for f in $$(grep -oE '\]\([a-zA-Z0-9/_.-]+\.md\)' docs/README.md | sed 's/^](//; s/)$$//'); do \
		[ -f "docs/$$f" ] || { echo "missing: docs/$$f"; missing=1; }; \
	done; \
	[ $$missing -eq 0 ] && echo "docs index OK: all linked files exist"
