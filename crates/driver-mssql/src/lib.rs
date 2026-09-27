//! MSSQL driver for Datara, implemented over Tiberius (TDS 7.3+, rustls).

mod convert;
mod driver;
mod queries;
mod session;

pub use convert::column_data_to_value;
pub use driver::{map_tiberius_error, MssqlDriver};
pub use session::{quote_ident, MssqlSession};
