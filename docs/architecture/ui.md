# UI architecture

Declarative Slint under `ui/`: `app.slint` (root `MainWindow`),
`theme.slint` (global `Theme` — Catppuccin-ish palette, dark/light), plus
planned `components/`, `pages/`, `dialogs/`, `editor/`, `grid/` subtrees.
Slint carries view state only; no business logic, no DB calls. Shortcuts bind
to `domain::Command`, never hard-coded keys in components.

Rendering: winit backend + femtovg, Wayland-native (X11 fallback via winit).
The UI thread never blocks on I/O — bridge spawns onto Tokio and posts back.

**Implemented:** root window and theme; launches under Wayland with app ID
`datara`. **Pending:** all functional components — phases 2–6.
