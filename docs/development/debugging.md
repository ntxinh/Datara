# Debugging

Logging is `tracing` + `tracing-subscriber` with `EnvFilter`; the default
filter is `info,datara=debug`. Override with `RUST_LOG`:

```sh
RUST_LOG=debug cargo run -p datara           # verbose app logs
RUST_LOG=winit=debug cargo run -p datara     # confirm the Wayland backend
```

Wayland: `main` sets `WINIT_UNIX_BACKEND=wayland` when `WAYLAND_DISPLAY` is
present and the user hasn't overridden the backend; unset it to exercise the
X11/XWayland path. Window app ID is `datara`.

## Verifying the Wayland app_id

From the running session (verified 2026-09-28 under niri, Task 8.1):

```sh
niri msg windows        # look for the Datara window → App ID: "datara"
```

`niri msg windows` reported `Title: "Datara", App ID: "datara"` — matching
`slint::set_xdg_app_id("datara")` in `main.rs` and `StartupWMClass=datara`
in the desktop entry. On compositors without a `niri msg` equivalent, run
`WAYLAND_DEBUG=1 datara` and check the `xdg_toplevel.set_app_id` argument
in the protocol trace.

There is no attached-debugger recipe yet — `gdb`/`lldb` on
`target/debug/datara` works as usual for a Rust binary.
