//! Data grid backing model: row cache, cell selection, and the
//! `slint::Model` adapter for virtualized row rendering.

mod cache;
mod model;

pub use cache::{CellSelection, RowCache};
pub use model::VirtualizedRows;
