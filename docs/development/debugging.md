# Debugging

Logging is `tracing` + `tracing-subscriber` with `EnvFilter`; the default
filter is `info,datara=debug`. Override with `RUST_LOG`:

```sh
RUST_LOG=debug cargo run -p datara           # verbose app logs
RUST_LOG=winit=debug cargo run -p datara     # confirm the Wayland backend
```

Wayland: `main` sets `WINIT_UNIX_BACKEND=wayland` when `WAYLAND_DISPLAY` is
present and the user hasn't overridden the backend; unset it to exercise the
X11/XWayland path. Window app ID is `datara` (checkable via compositor tools
like `niri msg windows`).

There is no attached-debugger recipe yet — `gdb`/`lldb` on
`target/debug/datara` works as usual for a Rust binary.
