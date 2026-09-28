# UI components

Planned Slint component tree (all under `ui/`):

| Component | File | Role | Status |
|---|---|---|---|
| `MainWindow` | `app.slint` | root window, toolbar/sidebar/editor/statusbar shell | implemented |
| `Theme` | `theme.slint` | color palette, dark/light | implemented |
| schema tree | `components/sidebar.slint` | connection browser | implemented — 3.1 |
| tabs | `components/tabs.slint` | editor tab strip | implemented — 4.2 |
| connection dialog | `dialogs/connection.slint` | profile editor | implemented — 2.4 |
| query editor | `editor/query_editor.slint` | SQL editing surface | implemented — 4.2 |
| result grid | `grid/result_grid.slint` | virtualized results | planned — phase 5 |
| history page | `pages/history.slint` | query history + saved queries | implemented — 6.1/6.2 |
| save dialog | `dialogs/save_query.slint` | name input for Ctrl+S | implemented — 6.2 |

Rule: components own view state only; actions go through `domain::Command`
and the app bridge.
