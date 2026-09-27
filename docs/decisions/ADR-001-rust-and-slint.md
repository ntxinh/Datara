# ADR-001: Rust + Slint

**Status:** accepted (2026-09-27)

## Context

Datara must be a native-feeling, fast Linux desktop app — not Electron.
Options considered: GTK (C/gtk-rs), Qt (C++/bindings), Iced, and Slint.

## Decision

Rust for all code; Slint for the declarative UI, rendered via winit +
femtovg, Wayland-native.

- Rust: memory safety without GC, strong async ecosystem (Tokio), mature
  database drivers (Tiberius for MSSQL).
- Slint: declarative component model suited to a data-heavy tool, real
  Wayland support through winit, small binary/runtime footprint, and an
  active upstream.

## Consequences

- **License is GPL-3.0.** Slint's free tier requires GPL; a proprietary
  license would need a paid Slint license. The whole project is GPL-3.0.
- Rust is pinned via `mise.toml` (1.98.1); Slint pinned to 1.18 with
  `backend-winit`, `renderer-femtovg`, `std`.
- UI is `.slint` files, not Rust widget code — enforces the view/logic
  boundary (see AGENTS.md architecture rules).
