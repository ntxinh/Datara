//! Materialized row cache plus rectangular cell selection.

use std::cmp::Ordering;

use datara_domain::{QueryColumn, QueryResult, QueryRow, Value};

/// A materialized snapshot of one `QueryResult`, ready for grid rendering.
#[derive(Debug)]
pub struct RowCache {
    columns: Vec<QueryColumn>,
    rows: Vec<QueryRow>,
    truncated: bool,
}

impl RowCache {
    /// Build a cache from a finished query result.
    pub fn from_result(r: QueryResult) -> Self {
        Self {
            columns: r.columns,
            rows: r.rows,
            truncated: r.truncated,
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Column headers (name + data type) for the grid's header row.
    pub fn columns(&self) -> &[QueryColumn] {
        &self.columns
    }

    /// Whether the result was truncated server-side.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Cell value at `(row, col)`; missing cells (ragged rows) read as `Null`.
    pub fn cell(&self, row: usize, col: usize) -> &Value {
        self.rows
            .get(row)
            .and_then(|r| r.cells.get(col))
            .unwrap_or(&Value::Null)
    }

    /// `(display text, is_null)` for one cell — what the grid needs to render.
    pub fn cell_text(&self, row: usize, col: usize) -> (String, bool) {
        match self.cell(row, col) {
            Value::Null => (String::new(), true),
            v => (v.to_string(), false),
        }
    }

    /// Whether the cell at `(row, col)` is `Value::Null`.
    pub fn is_null(&self, row: usize, col: usize) -> bool {
        matches!(self.cell(row, col), Value::Null)
    }

    /// Serialize the selection's rectangle as TSV: cells joined `\t`, rows `\n`,
    /// `NULL` becomes the empty string. Out-of-range cells are skipped.
    pub fn copy_cells(&self, sel: &CellSelection) -> String {
        let ((r0, c0), (r1, c1)) = sel.bounds();
        let r0 = r0.min(self.row_count());
        let r1 = r1.min(self.row_count());
        let c0 = c0.min(self.column_count());
        let c1 = c1.min(self.column_count());
        (r0..r1)
            .map(|r| {
                (c0..c1)
                    .map(|c| match self.cell(r, c) {
                        Value::Null => String::new(),
                        v => v.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Stable-sort rows by one column. `Int`/`Float` compare numerically
    /// (promoted to f64), everything else by display string. `Null` always
    /// sorts last, in both directions.
    pub fn sort_rows(&mut self, col: usize, ascending: bool) {
        self.rows.sort_by(|a, b| {
            let va = a.cells.get(col).unwrap_or(&Value::Null);
            let vb = b.cells.get(col).unwrap_or(&Value::Null);
            match (va, vb) {
                (Value::Null, Value::Null) => Ordering::Equal,
                (Value::Null, _) => Ordering::Greater, // nulls last, both directions
                (_, Value::Null) => Ordering::Less,
                _ => {
                    let ord = cmp_values(va, vb);
                    if ascending {
                        ord
                    } else {
                        ord.reverse()
                    }
                }
            }
        });
    }
}

/// Rectangular grid selection as anchor + head; `bounds()` normalizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSelection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl CellSelection {
    /// `((row_lo, col_lo), (row_hi_exclusive, col_hi_exclusive))` normalized
    /// so a reversed drag still yields a valid rectangle.
    pub fn bounds(&self) -> ((usize, usize), (usize, usize)) {
        let (r0, r1) = if self.anchor.0 <= self.head.0 {
            (self.anchor.0, self.head.0 + 1)
        } else {
            (self.head.0, self.anchor.0 + 1)
        };
        let (c0, c1) = if self.anchor.1 <= self.head.1 {
            (self.anchor.1, self.head.1 + 1)
        } else {
            (self.head.1, self.anchor.1 + 1)
        };
        ((r0, c0), (r1, c1))
    }
}

fn cmp_values(a: &Value, b: &Value) -> Ordering {
    match (numeric(a), numeric(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        _ => a.to_string().cmp(&b.to_string()),
    }
}

fn numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str) -> QueryColumn {
        QueryColumn {
            name: name.into(),
            data_type: "text".into(),
        }
    }

    fn row(cells: Vec<Value>) -> QueryRow {
        QueryRow { cells }
    }

    fn cache(cells: Vec<Vec<Value>>) -> RowCache {
        RowCache::from_result(QueryResult {
            columns: vec![col("a"), col("b")],
            rows: cells.into_iter().map(row).collect(),
            rows_affected: None,
            truncated: false,
        })
    }

    #[test]
    fn bounds_normalizes_reversed_selection() {
        let sel = CellSelection {
            anchor: (5, 3),
            head: (2, 1),
        };
        assert_eq!(sel.bounds(), ((2, 1), (6, 4)));

        let sel = CellSelection {
            anchor: (2, 1),
            head: (5, 3),
        };
        assert_eq!(sel.bounds(), ((2, 1), (6, 4)));
    }

    #[test]
    fn copy_cells_tsv_shape_and_null_empty() {
        let c = cache(vec![
            vec![Value::Int(1), Value::Text("x".into())],
            vec![Value::Null, Value::Text("y".into())],
            vec![Value::Int(3), Value::Null],
        ]);
        let sel = CellSelection {
            anchor: (0, 0),
            head: (2, 1),
        };
        assert_eq!(c.copy_cells(&sel), "1\tx\n\ty\n3\t");

        // Reversed selection copies the same rectangle.
        let rev = CellSelection {
            anchor: (2, 1),
            head: (0, 0),
        };
        assert_eq!(c.copy_cells(&rev), "1\tx\n\ty\n3\t");
    }

    #[test]
    fn copy_cells_clamps_out_of_range() {
        let c = cache(vec![vec![Value::Int(7), Value::Int(8)]]);
        let sel = CellSelection {
            anchor: (0, 0),
            head: (99, 99),
        };
        assert_eq!(c.copy_cells(&sel), "7\t8");
    }

    #[test]
    fn sort_numeric_column() {
        let mut c = cache(vec![
            vec![Value::Int(10), Value::Null],
            vec![Value::Int(2), Value::Null],
            vec![Value::Float(1.5), Value::Null],
        ]);
        c.sort_rows(0, true);
        let vals: Vec<String> = (0..3).map(|r| c.cell(r, 0).to_string()).collect();
        assert_eq!(vals, ["1.5", "2", "10"]);
    }

    #[test]
    fn sort_text_column_and_nulls_last() {
        let mut c = cache(vec![
            vec![Value::Text("banana".into()), Value::Null],
            vec![Value::Null, Value::Null],
            vec![Value::Text("apple".into()), Value::Null],
        ]);
        c.sort_rows(0, true);
        assert_eq!(c.cell(0, 0).to_string(), "apple");
        assert_eq!(c.cell(1, 0).to_string(), "banana");
        assert!(c.is_null(2, 0));

        // Descending still puts nulls last.
        c.sort_rows(0, false);
        assert_eq!(c.cell(0, 0).to_string(), "banana");
        assert_eq!(c.cell(1, 0).to_string(), "apple");
        assert!(c.is_null(2, 0));
    }

    #[test]
    fn truncated_flag_carried() {
        let r = QueryResult {
            columns: vec![col("a")],
            rows: vec![],
            rows_affected: None,
            truncated: true,
        };
        assert!(RowCache::from_result(r).truncated());
    }

    #[test]
    fn cell_text_marks_null() {
        let c = cache(vec![vec![Value::Int(5), Value::Null]]);
        assert_eq!(c.cell_text(0, 0), ("5".to_string(), false));
        assert_eq!(c.cell_text(0, 1), ("".to_string(), true));
    }
}
