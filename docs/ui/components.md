# UI components

Planned Slint component tree (all under `ui/`):

| Component | File | Role | Status |
|---|---|---|---|
| `MainWindow` | `app.slint` | root window, layout shell | implemented (placeholder) |
| `Theme` | `theme.slint` | color palette, dark/light | implemented |
| schema tree | `components/schema_tree.slint` | connection browser | planned — phase 3 |
| toolbar / status bar / tabs | `components/{toolbar,status_bar,tabs}.slint` | chrome | planned — phase 4 |
| connection dialog | `dialogs/connection.slint` | profile editor | planned — phase 2 |
| query editor | `editor/query_editor.slint` | SQL editing surface | planned — phase 4 |
| result grid | `grid/result_grid.slint` | virtualized results | planned — phase 5 |
| history page | `pages/history.slint` | query history | planned — phase 6 |

Rule: components own view state only; actions go through `domain::Command`
and the app bridge.
