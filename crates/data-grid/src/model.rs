//! `slint::Model` adapter over `RowCache` — materializes `SharedString`s only
//! for the rows the view asks for.

use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::RowCache;

/// Virtualized row model for the grid's `ListView`. Each row's `Data` is a
/// `ModelRc<SharedString>` of cell text; `is_null_cell` provides the flag for
/// null styling (task 5.2 wraps it into the slint-side row struct).
pub struct VirtualizedRows {
    cache: Rc<RowCache>,
}

impl VirtualizedRows {
    pub fn new(cache: Rc<RowCache>) -> Self {
        Self { cache }
    }

    /// Whether the cell at `(row, col)` is `Value::Null` (for cell styling).
    pub fn is_null_cell(&self, row: usize, col: usize) -> bool {
        self.cache.is_null(row, col)
    }
}

impl slint::Model for VirtualizedRows {
    type Data = ModelRc<SharedString>;

    fn row_count(&self) -> usize {
        self.cache.row_count()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        (row < self.cache.row_count()).then(|| {
            let cells: Vec<SharedString> = (0..self.cache.column_count())
                .map(|c| SharedString::from(self.cache.cell_text(row, c).0))
                .collect();
            ModelRc::new(VecModel::from(cells))
        })
    }
    // ponytail: no ModelNotify — the app replaces the model when results change
    // (Task 5.2/5.3 re-set the property after sort/new query).
    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &()
    }
}

#[cfg(test)]
mod tests {
    use datara_domain::{QueryColumn, QueryResult, QueryRow, Value};
    use slint::Model;

    use super::*;

    fn cache() -> Rc<RowCache> {
        Rc::new(RowCache::from_result(QueryResult {
            columns: vec![
                QueryColumn {
                    name: "a".into(),
                    data_type: "int".into(),
                },
                QueryColumn {
                    name: "b".into(),
                    data_type: "text".into(),
                },
            ],
            rows: vec![
                QueryRow {
                    cells: vec![Value::Int(42), Value::Text("hi".into())],
                },
                QueryRow {
                    cells: vec![Value::Null, Value::Bool(true)],
                },
            ],
            rows_affected: None,
            truncated: false,
        }))
    }

    #[test]
    fn row_data_matches_value_display() {
        let m = VirtualizedRows::new(cache());
        assert_eq!(Model::row_count(&m), 2);

        let r0 = m.row_data(0).unwrap();
        assert_eq!(r0.row_count(), 2);
        assert_eq!(r0.row_data(0).unwrap(), SharedString::from("42"));
        assert_eq!(r0.row_data(1).unwrap(), SharedString::from("hi"));

        // Null renders as empty text; is_null_cell carries the flag.
        let r1 = m.row_data(1).unwrap();
        assert_eq!(r1.row_data(0).unwrap(), SharedString::from(""));
        assert!(m.is_null_cell(1, 0));
        assert!(!m.is_null_cell(1, 1));

        assert!(m.row_data(2).is_none());
    }
}
