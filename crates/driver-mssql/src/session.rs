//! The [`DatabaseSession`] implementation wrapping a Tiberius client.

use async_trait::async_trait;
use datara_database::DatabaseSession;
use datara_domain::{
    ColumnInfo, DatabaseInfo, IndexInfo, QueryColumn, QueryResult, QueryRow, Result, SchemaInfo,
    TableDescription, TableInfo, TableKind,
};
use futures_util::TryStreamExt;
use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use tiberius::{Client, QueryItem};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::Compat;

use crate::convert::column_data_to_value;
use crate::driver::map_tiberius_error;
use crate::queries;

/// An open MSSQL session. The client is behind a `Mutex` because the TDS
/// protocol is half-duplex: only one request may be in flight per connection.
pub struct MssqlSession {
    client: Mutex<Client<Compat<TcpStream>>>,
}

/// True when the parsed batch returns rows. Unparseable SQL (server-specific
/// syntax) defaults to `true` — the server decides, the parser never blocks
/// a valid statement.
fn is_row_returning(query: &str) -> bool {
    match Parser::parse_sql(&GenericDialect {}, query) {
        Ok(statements) => statements.iter().all(|s| matches!(s, Statement::Query(_))),
        Err(_) => true,
    }
}

/// Quote `ident` as a `[` … `]` delimited identifier, escaping `]` as `]]`.
/// Free-standing so callers without a session handle (preview SQL) reuse the
/// one rule.
pub fn quote_ident(ident: &str) -> String {
    format!("[{}]", ident.replace(']', "]]"))
}

/// Drain a [`tiberius::QueryStream`]'s first result set into a [`QueryResult`],
/// collecting at most `max_rows` rows. Draining continues past the cap (rows
/// are discarded) so the TDS token stream ends cleanly — abandoning the stream
/// mid-flight would require an attention/cancel packet.
async fn drain_rows(
    stream: &mut tiberius::QueryStream<'_>,
    max_rows: usize,
) -> std::result::Result<QueryResult, tiberius::error::Error> {
    let mut columns: Vec<QueryColumn> = Vec::new();
    let mut rows: Vec<QueryRow> = Vec::new();
    let mut truncated = false;

    while let Some(item) = stream.try_next().await? {
        match item {
            QueryItem::Metadata(meta) if meta.result_index() == 0 => {
                columns = meta
                    .columns()
                    .iter()
                    .map(|c| QueryColumn {
                        name: c.name().to_owned(),
                        // ponytail: debug name for the type; map to friendly
                        // names when the type display matters to users.
                        data_type: format!("{:?}", c.column_type()),
                    })
                    .collect();
            }
            QueryItem::Row(row) if row.result_index() == 0 => {
                if rows.len() == max_rows {
                    truncated = true;
                    // Keep draining but discard (see fn docs).
                } else {
                    rows.push(QueryRow {
                        cells: row
                            .cells()
                            .map(|(_, data)| column_data_to_value(data))
                            .collect(),
                    });
                }
            }
            // Later result sets of a multi-statement batch are drained and
            // ignored; the grid shows the first result set.
            _ => {}
        }
    }

    Ok(QueryResult {
        columns,
        rows,
        rows_affected: None,
        truncated,
    })
}

impl MssqlSession {
    pub(crate) fn new(client: Client<Compat<TcpStream>>) -> Self {
        Self {
            client: Mutex::new(client),
        }
    }

    /// Issue `USE [database]` on `client`. Caller must hold the lock.
    async fn switch_db(client: &mut Client<Compat<TcpStream>>, database: &str) -> Result<()> {
        client
            .execute(format!("USE {}", quote_ident(database)), &[])
            .await
            .map_err(map_tiberius_error)?;
        Ok(())
    }

    async fn name_list(client: &mut Client<Compat<TcpStream>>, sql: &str) -> Result<Vec<String>> {
        let rows = client
            .simple_query(sql)
            .await
            .map_err(map_tiberius_error)?
            .into_first_result()
            .await
            .map_err(map_tiberius_error)?;
        Ok(rows
            .iter()
            .map(|r| r.get::<&str, usize>(0).unwrap_or_default().to_owned())
            .collect())
    }
}

#[async_trait]
impl DatabaseSession for MssqlSession {
    async fn list_databases(&self) -> Result<Vec<DatabaseInfo>> {
        let mut client = self.client.lock().await;
        Ok(Self::name_list(&mut client, queries::LIST_DATABASES)
            .await?
            .into_iter()
            .map(|name| DatabaseInfo { name })
            .collect())
    }

    async fn list_schemas(&self, database: &str) -> Result<Vec<SchemaInfo>> {
        let mut client = self.client.lock().await;
        Self::switch_db(&mut client, database).await?;
        Ok(Self::name_list(&mut client, queries::LIST_SCHEMAS)
            .await?
            .into_iter()
            .map(|name| SchemaInfo { name })
            .collect())
    }

    async fn list_tables(&self, database: &str, schema: &str) -> Result<Vec<TableInfo>> {
        let mut client = self.client.lock().await;
        Self::switch_db(&mut client, database).await?;
        let rows = client
            .query(queries::LIST_TABLES, &[&schema])
            .await
            .map_err(map_tiberius_error)?
            .into_first_result()
            .await
            .map_err(map_tiberius_error)?;
        Ok(rows
            .iter()
            .map(|r| TableInfo {
                schema: r.get::<&str, usize>(0).unwrap_or_default().to_owned(),
                name: r.get::<&str, usize>(1).unwrap_or_default().to_owned(),
                kind: if r.get::<&str, usize>(2) == Some("view") {
                    TableKind::View
                } else {
                    TableKind::Base
                },
            })
            .collect())
    }

    async fn describe_table(
        &self,
        database: &str,
        schema: &str,
        table: &str,
    ) -> Result<TableDescription> {
        let mut client = self.client.lock().await;
        Self::switch_db(&mut client, database).await?;

        let col_rows = client
            .query(queries::DESCRIBE_COLUMNS, &[&schema, &table])
            .await
            .map_err(map_tiberius_error)?
            .into_first_result()
            .await
            .map_err(map_tiberius_error)?;
        let columns = col_rows
            .iter()
            .map(|r| ColumnInfo {
                name: r.get::<&str, usize>(0).unwrap_or_default().to_owned(),
                data_type: r.get::<&str, usize>(1).unwrap_or_default().to_owned(),
                nullable: r.get::<bool, usize>(2).unwrap_or(false),
                is_primary_key: r.get::<i32, usize>(3).map(|v| v != 0).unwrap_or(false),
                ordinal: r.get::<i32, usize>(4).unwrap_or_default().max(0) as u32,
            })
            .collect();

        let idx_rows = client
            .query(queries::DESCRIBE_INDEXES, &[&schema, &table])
            .await
            .map_err(map_tiberius_error)?
            .into_first_result()
            .await
            .map_err(map_tiberius_error)?;
        // Ordered by index name, so equal names are contiguous — group by
        // scanning, accumulating columns into the last seen index.
        let mut indexes: Vec<IndexInfo> = Vec::new();
        for r in &idx_rows {
            let name = r.get::<&str, usize>(0).unwrap_or_default().to_owned();
            let is_unique = r.get::<bool, usize>(1).unwrap_or(false);
            let column = r.get::<&str, usize>(2).unwrap_or_default().to_owned();
            match indexes.last_mut() {
                Some(idx) if idx.name == name => idx.columns.push(column),
                _ => indexes.push(IndexInfo {
                    name,
                    columns: vec![column],
                    is_unique,
                }),
            }
        }

        Ok(TableDescription {
            schema: schema.to_owned(),
            name: table.to_owned(),
            columns,
            indexes,
        })
    }

    async fn execute(&self, database: &str, query: &str, max_rows: usize) -> Result<QueryResult> {
        let mut client = self.client.lock().await;
        Self::switch_db(&mut client, database).await?;

        if is_row_returning(query) {
            let mut stream = client.query(query, &[]).await.map_err(map_tiberius_error)?;
            drain_rows(&mut stream, max_rows)
                .await
                .map_err(map_tiberius_error)
        } else {
            let result = client
                .execute(query, &[])
                .await
                .map_err(map_tiberius_error)?;
            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                rows_affected: Some(result.total()),
                truncated: false,
            })
        }
    }

    async fn cancel(&self) -> Result<()> {
        // Cancellation is handled by the service layer: aborting the spawned
        // task drops the TCP stream, which terminates the server-side request.
        Ok(())
    }

    fn quote_ident(&self, ident: &str) -> String {
        quote_ident(ident)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_escapes_brackets() {
        assert_eq!(quote_ident("a"), "[a]");
        assert_eq!(quote_ident("a]b"), "[a]]b]");
        assert_eq!(quote_ident(""), "[]");
    }

    #[test]
    fn statement_classification() {
        assert!(is_row_returning("SELECT 1"));
        assert!(is_row_returning("SELECT 1; SELECT 2"));
        assert!(!is_row_returning("UPDATE t SET a = 1"));
        assert!(!is_row_returning("CREATE TABLE t (a int)"));
        assert!(!is_row_returning("DELETE FROM t"));
        assert!(!is_row_returning("INSERT INTO t VALUES (1)"));
        // Unparseable (server-specific) syntax falls back to the query path.
        assert!(is_row_returning("some @@garbage$$"));
    }
}
