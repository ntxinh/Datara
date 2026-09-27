# Application architecture

`datara-app` owns the process: clap parses `gui` (default) and `mcp-serve`.
The GUI path initializes tracing (`EnvFilter`, default `info,datara=debug`),
compiles `ui/app.slint` via `slint-build`, and runs `MainWindow`. A bridge
module will translate Slint callbacks into `domain::Command` values and spawn
service calls on Tokio, delivering results through
`slint::invoke_from_event_loop`.

**Implemented:** CLI, tracing init, window launch, Wayland backend selection
(`WINIT_UNIX_BACKEND=wayland` when `WAYLAND_DISPLAY` is set).
**Pending:** service wiring, bridge, command dispatch — phases 2–4.
