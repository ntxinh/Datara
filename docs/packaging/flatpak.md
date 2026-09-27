# Flatpak packaging

`packaging/flatpak/io.github.ntxinh.Datara.yml` builds from the repo with
cargo vendored via `cargo-sources.json` (generated for a clean offline
build). Sandbox permissions stay minimal: Wayland socket, session-bus access
to Secret Service (`org.freedesktop.secrets`), and network for database
connections. No home/filesystem access — state lives in the XDG dirs inside
the sandbox.

App ID `io.github.ntxinh.Datara` matches the desktop entry, metainfo, and
icon names.

**Implemented:** nothing — manifest pending. **Pending:** phase 8
(task 8.3).
