//! Result grid state (Task 5.2): owns the `RowCache` of the last result,
//! column widths, and the rectangular selection; pushes `GridCol`/`GridRow`
//!/`GridSel` models to the `Bridge` global.
//!
//! Kept `Send`: Slint `ModelRc`s are rebuilt from plain data on every
//! mutation (no `Rc<VecModel>` stored) so `UiCtx` can cross into Tokio
//! tasks.

use datara_data_grid::{CellSelection, RowCache};
use datara_domain::QueryResult;
use slint::{ModelRc, SharedString, VecModel};

use crate::{Bridge, GridCell, GridCol, GridRow, GridSel};

/// Minimum column width for the resize handle.
pub const MIN_COL_WIDTH: i32 = 40;
/// First-render column width.
const DEFAULT_COL_WIDTH: i32 = 140;

/// Everything the grid needs after the last `QueryResult` event.
#[derive(Default)]
pub struct GridState {
    cache: Option<RowCache>,
    widths: Vec<i32>,
    sel: Option<CellSelection>,
}

impl GridState {
    /// Rows in the cached result — headless smoke tests poll this to see
    /// a dispatched `QueryResult` land without touching the Slint model.
    #[cfg(test)]
    pub(crate) fn cached_rows(&self) -> usize {
        self.cache.as_ref().map_or(0, |c| c.row_count())
    }

    /// Install a finished result: cache it, size columns to a default
    /// width, push both models and the footer text; reset selection/sort.
    pub fn set_result(&mut self, bridge: &Bridge, result: QueryResult, info: String) {
        let cache = RowCache::from_result(result);
        self.widths = vec![DEFAULT_COL_WIDTH; cache.column_count()];
        self.sel = None;
        Self::push_rows(&cache, bridge);
        self.cache = Some(cache);
        self.push_cols(bridge);
        bridge.set_selection(sel_prop(None));
        bridge.set_sort_col(-1);
        bridge.set_result_info(info.into());
    }

    /// A new query started (or errored/cancelled): drop the grid contents.
    /// `info` still shows the status text (error message / "Running…").
    pub fn clear(&mut self, bridge: &Bridge, info: &str) {
        self.cache = None;
        self.widths.clear();
        self.sel = None;
        bridge.set_columns(ModelRc::new(VecModel::from(Vec::<GridCol>::new())));
        bridge.set_rows(ModelRc::new(VecModel::from(Vec::<GridRow>::new())));
        bridge.set_grid_w(0);
        bridge.set_selection(sel_prop(None));
        bridge.set_result_info(info.into());
    }

    /// Cell click: anchor a fresh 1×1 selection.
    pub fn select(&mut self, bridge: &Bridge, row: i32, col: i32) {
        let Some(cache) = &self.cache else { return };
        if row < 0 || col < 0 {
            return;
        }
        let (r, c) = (row as usize, col as usize);
        if r >= cache.row_count() || c >= cache.column_count() {
            return;
        }
        self.sel = Some(CellSelection {
            anchor: (r, c),
            head: (r, c),
        });
        bridge.set_selection(sel_prop(self.sel));
    }

    /// Drag update: move the selection head. `row` already includes the
    /// pointer's row delta (computed .slint-side); `x` is the pointer's x
    /// within the row — resolve to a column via the width prefix sums.
    pub fn drag(&mut self, bridge: &Bridge, row: i32, x: f32) {
        // Resolve before the mutable borrow: col_at only needs widths.
        let c = Self::col_at(&self.widths, x);
        let (Some(cache), Some(sel)) = (&self.cache, self.sel.as_mut()) else {
            return;
        };
        sel.head = (
            row.clamp(0, cache.row_count().saturating_sub(1) as i32) as usize,
            c,
        );
        bridge.set_selection(sel_prop(self.sel));
    }

    /// Column index under row-space `x`; last column past the end.
    fn col_at(widths: &[i32], x: f32) -> usize {
        let mut acc = 0f32;
        let last = widths.len().saturating_sub(1);
        for (i, w) in widths.iter().enumerate() {
            acc += *w as f32;
            if x < acc {
                return i;
            }
        }
        last
    }

    /// Resize handle released: `delta` is the cumulative drag distance.
    /// Rejects out-of-range `idx` rather than silently resizing column 0.
    pub fn resize(&mut self, bridge: &Bridge, idx: i32, delta: f32) {
        if idx < 0 || idx as usize >= self.widths.len() {
            return;
        }
        let w = &mut self.widths[idx as usize];
        *w = (*w + delta as i32).max(MIN_COL_WIDTH);
        self.push_cols(bridge);
    }

    /// Toggle-sort a column (re-clicking flips direction; a different
    /// column starts ascending). Wired now; `RowCache::sort_rows` exists
    /// since 5.1 so the indicator isn't a dead control.
    pub fn sort(&mut self, bridge: &Bridge, idx: i32) {
        let Some(cache) = &mut self.cache else { return };
        if idx < 0 || idx as usize >= cache.column_count() {
            return;
        }
        let asc = if bridge.get_sort_col() == idx {
            !bridge.get_sort_asc()
        } else {
            true
        };
        cache.sort_rows(idx as usize, asc);
        bridge.set_sort_col(idx);
        bridge.set_sort_asc(asc);
        Self::push_rows(cache, bridge);
        // Selection coords refer to row indices, which just changed —
        // drop it rather than highlight the wrong cells.
        self.sel = None;
        bridge.set_selection(sel_prop(None));
    }

    /// Ctrl+C: copy the selection rectangle as TSV to the system clipboard.
    /// `clipboard` is the app-lifetimes holder in `UiCtx` (created lazily).
    pub fn copy(&mut self, clipboard: &mut Option<arboard::Clipboard>, bridge: &Bridge) {
        let (Some(cache), Some(sel)) = (&self.cache, self.sel) else {
            return;
        };
        let tsv = cache.copy_cells(&sel);
        if tsv.is_empty() {
            return;
        }
        let ((r0, c0), (r1, c1)) = sel.bounds();
        // Cell count of the in-bounds rectangle for the status line.
        let n = r1
            .saturating_sub(r0)
            .min(cache.row_count().saturating_sub(r0))
            * c1.saturating_sub(c0)
                .min(cache.column_count().saturating_sub(c0));
        if Self::copy_to_clipboard(clipboard, bridge, tsv).is_some() {
            bridge.set_status(format!("Copied {n} cells").into());
        }
    }

    /// Copy `text` to the system clipboard, reporting failures on the status
    /// line. `slot` lazily holds the `arboard::Clipboard` — on Wayland it owns
    /// the data-control object that keeps copied text alive past this call,
    /// so the holder must live as long as the app. `Some` on success.
    pub(crate) fn copy_to_clipboard(
        slot: &mut Option<arboard::Clipboard>,
        bridge: &Bridge,
        text: String,
    ) -> Option<()> {
        if slot.is_none() {
            *slot = arboard::Clipboard::new().ok();
        }
        match slot.as_mut().map(|c| c.set_text(text)) {
            Some(Ok(())) => Some(()),
            Some(Err(e)) => {
                bridge.set_status(format!("Copy failed: {e}").into());
                None
            }
            None => {
                bridge.set_status("Copy failed: no clipboard provider".into());
                None
            }
        }
    }

    /// `cache.columns()` + `widths` → the `GridCol` model.
    fn push_cols(&self, bridge: &Bridge) {
        let cols: Vec<GridCol> = self
            .cache
            .iter()
            .flat_map(|c| c.columns().iter().zip(&self.widths))
            .map(|(qc, w)| GridCol {
                name: qc.name.clone().into(),
                data_type: qc.data_type.clone().into(),
                width: *w,
            })
            .collect();
        bridge.set_grid_w(self.widths.iter().sum());
        bridge.set_columns(ModelRc::new(VecModel::from(cols)));
    }

    /// Rebuild the `GridRow` model from the cache. Associated fn (not
    /// `&self`) so `sort` can call it while `cache` is mutably borrowed.
    ///
    /// ponytail: eager materialization per query — bounded by
    /// `default_limit` + truncation (≤1000 rows). `VirtualizedRows` exists
    /// as the lazy adapter but yields SharedString-only rows (no is-null);
    /// switch if limits grow past what eager VecModel handles smoothly.
    fn push_rows(cache: &RowCache, bridge: &Bridge) {
        let ncols = cache.column_count();
        let rows: Vec<GridRow> = (0..cache.row_count())
            .map(|r| GridRow {
                cells: ModelRc::new(VecModel::from(
                    (0..ncols)
                        .map(|c| {
                            let (text, is_null) = cache.cell_text(r, c);
                            GridCell {
                                // "NULL" is rendered .slint-side so display
                                // text and TSV copy stay consistent.
                                text: SharedString::from(text),
                                is_null,
                            }
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect();
        bridge.set_rows(ModelRc::new(VecModel::from(rows)));
    }
}

/// `CellSelection` (usize, exclusive-normalized bounds) → `GridSel`
/// (inclusive ends for the delegate's `>=`/`<=` checks).
fn sel_prop(sel: Option<CellSelection>) -> GridSel {
    match sel {
        Some(s) => {
            let ((r0, c0), (r1, c1)) = s.bounds();
            GridSel {
                r0: r0 as i32,
                c0: c0 as i32,
                r1: r1 as i32 - 1,
                c1: c1 as i32 - 1,
                active: true,
            }
        }
        None => GridSel {
            active: false,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use datara_domain::{QueryColumn, QueryRow, Value};

    use super::*;

    fn result() -> QueryResult {
        QueryResult {
            columns: vec![
                QueryColumn {
                    name: "id".into(),
                    data_type: "int".into(),
                },
                QueryColumn {
                    name: "name".into(),
                    data_type: "nvarchar".into(),
                },
            ],
            rows: vec![
                QueryRow {
                    cells: vec![Value::Int(1), Value::Text("a".into())],
                },
                QueryRow {
                    cells: vec![Value::Int(2), Value::Null],
                },
            ],
            rows_affected: None,
            truncated: false,
        }
    }

    /// Column widths → prefix-sum lookup: left edge picks the column,
    /// interior picks it too, past the end clamps to the last column.
    #[test]
    fn col_at_resolves_by_width_prefix_sums() {
        let w = [100, 50, 200];
        assert_eq!(GridState::col_at(&w, 0.0), 0);
        assert_eq!(GridState::col_at(&w, 99.0), 0);
        assert_eq!(GridState::col_at(&w, 100.0), 1);
        assert_eq!(GridState::col_at(&w, 149.0), 1);
        assert_eq!(GridState::col_at(&w, 150.0), 2);
        assert_eq!(
            GridState::col_at(&w, 10_000.0),
            2,
            "past end clamps to last column"
        );
        assert_eq!(GridState::col_at(&[], 0.0), 0, "no columns → 0");
    }

    /// `is_null` must reach `GridCell` so .slint styles NULL cells;
    /// ragged/missing cells read as null too.
    #[test]
    fn is_null_maps_to_grid_cell() {
        let cache = RowCache::from_result(result());
        let (text, is_null) = cache.cell_text(1, 1);
        assert_eq!(text, "");
        assert!(is_null);
        let (text, is_null) = cache.cell_text(0, 1);
        assert_eq!(text, "a");
        assert!(!is_null);
        assert!(cache.is_null(9, 9));
    }

    /// TSV copy of a 2×2 selection carries the NULL as an empty cell.
    #[test]
    fn copy_rect_serializes_tsv() {
        let cache = RowCache::from_result(result());
        let sel = CellSelection {
            anchor: (0, 0),
            head: (1, 1),
        };
        assert_eq!(cache.copy_cells(&sel), "1\ta\n2\t");
    }
}
