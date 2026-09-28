# Flatpak packaging

`packaging/flatpak/io.github.ntxinh.Datara.yml` builds the app with
`flatpak-builder` on `org.freedesktop.Platform` 25.08 /
`org.freedesktop.Sdk` plus the `org.freedesktop.Sdk.Extension.rust-stable`
extension (provides cargo/rustc in the SDK sandbox).

## Dependencies

All crates are vendored via `packaging/flatpak/cargo-sources.json`,
generated from `Cargo.lock` with `flatpak-cargo-generator.py`
(`flatpak-builder-tools`, cargo subdir). Regenerate it whenever
`Cargo.lock` changes:

```sh
uv run flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
```

The generated sources land in `cargo/vendor/` inside the build sandbox
with a `cargo/config` redirecting crates.io to `vendored-sources`, so the
build is fully offline (`cargo build --offline --locked`).

## Build

```sh
make flatpak
```

Fetches the SDK + rust extension from flathub on first run
(`--install-deps-from`), builds, exports `packaging/flatpak/repo`, and
produces `datara.flatpak`. To build without the host `flatpak-builder`
package, use the Flatpak `org.flatpak.Builder` app instead:

```sh
flatpak run org.flatpak.Builder --force-clean --install-deps-from=flathub \
    packaging/flatpak/build packaging/flatpak/io.github.ntxinh.Datara.yml
```

## Sandbox permissions

| Permission | Why |
|---|---|
| `--socket=wayland` | winit + femtovg EGL on Wayland |
| `--socket=fallback-x11` | X11 fallback when Wayland is absent |
| `--device=dri` | hardware GL for femtovg; without it rendering falls back to software EGL (llvmpipe) |
| `--share=network` | TCP connections to SQL Server |
| `--talk-name=org.freedesktop.secrets` | keyring via Secret Service D-Bus |

**No filesystem permissions.** Host `$HOME` access is not granted; the
sandbox maps XDG `data`/`config`/`state` dirs to
`~/.var/app/io.github.ntxinh.Datara/{data,config,state}` and the `dirs`
crate resolves those XDG vars, which flatpak sets. App state therefore
lives under `~/.var/app/io.github.ntxinh.Datara/`, not
`~/.local/{share,config}/datara`.

## CI

`release.yml` has a `flatpak` job using
`flatpak/flatpak-github-actions@v8` (the `bilelmoussaoui` action was
renamed upstream).

**Implemented:** manifest, vendored cargo sources, `make flatpak`
target, release job. **Verified:** local `flatpak-builder` build — see
the task 8.3 report for proof status.
