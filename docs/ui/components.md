# UI components

Slint component tree (all under `ui/`):

| Component | File | Role |
|---|---|---|
| `MainWindow` | `app.slint` | root window: toolbar, sidebar+drag handle, tab strip, editor/split handle/grid panes, status bar, modal overlays |
| `Theme` | `theme.slint` | Catppuccin-ish palette; `dark` set from `[appearance] theme` at startup |
| `Sidebar` | `components/sidebar.slint` | connection browser: schema tree + filter LineEdit |
| `TabStrip` | `components/tabs.slint` | editor tab strip |
| `Palette` | `components/palette.slint` | command palette overlay (Ctrl+P) |
| `QueryEditor` | `editor/query_editor.slint` | TextInput + gutter + syntax-highlight overlay + completion popup |
| `ResultGrid` | `grid/result_grid.slint` | virtualized results: sortable/resizable columns, cell selection, Ctrl+C |
| `HistoryPanel` | `pages/history.slint` | query history + saved queries bottom pane |
| `ConnDialog` | `dialogs/connection.slint` | connection profile editor (test + save) |
| `SaveQueryDialog` | `dialogs/save_query.slint` | name input for Ctrl+S |

Rule: components own view state only; actions go through `domain::Command`
and `Bridge`.

## `Bridge` (ui/bridge.slint)

Single `global` — the only Slint ↔ Rust channel. Callbacks fire on the UI
thread; Rust pushes `in`/`in-out` properties back via
`invoke_from_event_loop`.

Callbacks (UI → Rust):

- **Connections/tree:** `new-connection`, `test-connection`, `toggle-node`,
  `open-table`, `filter-tree`
- **Editor:** `editor-changed`, `cursor-changed`, `insert-tab` (Tab →
  `[editor] tab_size` spaces), `completion-action`, `command` (key router,
  returns consumed)
- **Palette:** `palette-edited`, `palette-submit`, `palette-close`
- **Tabs:** `new-tab`, `close-tab`, `switch-tab`
- **Grid:** `grid-select`, `grid-drag`, `copy-selection`, `resize-column`,
  `sort-column`
- **History:** `history-search`, `history-rerun`, `history-copy`,
  `history-delete`, `history-toggle`
- **Saved queries:** `saved-search`, `save-query`, `saved-open`,
  `saved-delete`
- **Execution:** `cancel-query`

Pushed properties (Rust → UI): `tree`, `status`, `test-result`,
`editor-text`, `editor-font-size`, `line-count`, `cursor-position`,
`highlight-spans`, `set-cursor`, `schema-filter-visible`,
`palette-{visible,query,items,index}`, `conn-dialog-visible`, `columns`,
`rows`, `grid-w`, `result-info`, `selection`, `sort-col`, `sort-asc`,
`history-{visible,items,query,tab}`, `saved-items`, `save-dialog-visible`,
`completions`, `completion-index`, `query-running`, `tabs`, `active-tab`,
`sidebar-width`, `editor-split` (persisted to `[workspace]` on close).
