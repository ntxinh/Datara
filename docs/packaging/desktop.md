# Desktop integration

Desktop assets live in `packaging/share/` and `assets/icons/`, all keyed
to app ID `io.github.ntxinh.Datara`:

- `io.github.ntxinh.Datara.desktop` — desktop entry, `StartupWMClass=datara`.
- `io.github.ntxinh.Datara.metainfo.xml` — AppStream metainfo.
- `assets/icons/datara.svg` — icon source (rounded square `#89b4fa`,
  white 3×3 table-grid glyph) plus PNG renders at 32–512 px.

## StartupWMClass

`crates/app/src/main.rs` calls `slint::set_xdg_app_id("datara")` before the
window is shown, so the Wayland `app_id` / X11 `WM_CLASS` is `datara`
regardless of binary path or launch environment. Keep
`StartupWMClass=datara` in sync if that call ever changes.

## Window icon

`ui/app.slint` sets `Window.icon` to `assets/icons/datara-128.png`
(`@image-url` embeds it in the binary — no runtime file needed).

## Local dev install

```sh
./packaging/install-local.sh
```

installs the `.desktop`, metainfo, and hicolor icons under
`~/.local/share/` (plus `target/release/datara` into `~/.local/bin` when
present). `packaging/rpm/` (task 8.2) and `packaging/flatpak/` (task 8.3)
reuse these same files under system prefixes.

**Implemented:** desktop entry, metainfo, icons, install script, window
icon (phase 8, task 8.1). **Pending:** RPM spec (8.2), Flatpak manifest
(8.3).
