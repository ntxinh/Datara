use thiserror::Error;

fn query_details(error_number: Option<i32>, line: Option<u32>, column: Option<u32>) -> String {
    let mut parts = Vec::new();
    if let Some(n) = error_number {
        parts.push(format!("error {n}"));
    }
    if let Some(l) = line {
        parts.push(format!("line {l}"));
    }
    if let Some(c) = column {
        parts.push(format!("column {c}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

/// Errors surfaced to the user. `Display` output is user-readable (§27) and
/// must never contain credential material.
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("Connection failed: {message}")]
    Connection { message: String },
    #[error("Authentication failed: {message}")]
    Authentication { message: String },
    #[error("TLS error: {message}")]
    Tls { message: String },
    #[error(
        "Query failed: {message}{}",
        query_details(*error_number, *line, *column)
    )]
    Query {
        message: String,
        error_number: Option<i32>,
        line: Option<u32>,
        column: Option<u32>,
    },
    #[error("Schema error: {message}")]
    Schema { message: String },
    #[error("Storage error: {message}")]
    Storage { message: String },
    #[error("Secret store error: {message}")]
    Secret { message: String },
    #[error("Configuration error: {message}")]
    Config { message: String },
    #[error("Operation cancelled")]
    Cancelled,
    #[error("Driver error: {message}")]
    Driver { message: String },
}

pub type Result<T, E = DomainError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_error_display_strings() {
        assert_eq!(
            DomainError::Connection {
                message: "could not connect to host:1433".into()
            }
            .to_string(),
            "Connection failed: could not connect to host:1433"
        );
        assert_eq!(
            DomainError::Authentication {
                message: "login failed".into()
            }
            .to_string(),
            "Authentication failed: login failed"
        );
        assert_eq!(
            DomainError::Query {
                message: "incorrect syntax".into(),
                error_number: Some(102),
                line: Some(3),
                column: Some(7),
            }
            .to_string(),
            "Query failed: incorrect syntax (error 102, line 3, column 7)"
        );
        assert_eq!(
            DomainError::Query {
                message: "boom".into(),
                error_number: None,
                line: None,
                column: None,
            }
            .to_string(),
            "Query failed: boom"
        );
        assert_eq!(DomainError::Cancelled.to_string(), "Operation cancelled");
    }
}
