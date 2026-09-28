//! The [`DatabaseDriver`] implementation for Microsoft SQL Server (Tiberius).

use std::time::Duration;

use async_trait::async_trait;
use datara_database::{DatabaseDriver, DatabaseSession};
use datara_domain::{ConnectionProfile, Credentials, DomainError, EncryptionMode, Result};
use secrecy::ExposeSecret;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::session::MssqlSession;

/// TCP connect timeout; a hanging connect otherwise blocks the connect call
/// indefinitely (spec: connection timeout 10 s).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// SQL Server error numbers that mean authentication failure (login failed /
/// untrusted domain / SSPI handshake).
const AUTH_ERROR_CODES: [u32; 3] = [18456, 18452, 18453];

/// MSSQL driver. Stateless unit type — each `connect` opens a fresh session.
#[derive(Debug, Clone, Copy, Default)]
pub struct MssqlDriver;

#[async_trait]
impl DatabaseDriver for MssqlDriver {
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Box<dyn DatabaseSession>> {
        connect_inner(profile, credentials, CONNECT_TIMEOUT).await
    }
}

/// TCP connect + Tiberius handshake (prelogin, TLS, login) — the whole
/// thing under `timeout`, not just the socket: a peer that accepts TCP but
/// stalls prelogin would otherwise park `connect` forever and leave the
/// caller's session bookkeeping hanging.
async fn connect_inner(
    profile: &ConnectionProfile,
    credentials: &Credentials,
    timeout: Duration,
) -> Result<Box<dyn DatabaseSession>> {
    let mut config = Config::new();
    config.host(&profile.host);
    config.port(profile.port);
    if let Some(db) = &profile.database {
        config.database(db);
    }
    config.authentication(AuthMethod::sql_server(
        &credentials.username,
        credentials.password.expose_secret(),
    ));
    config.encryption(match profile.encryption {
        EncryptionMode::Disabled => EncryptionLevel::Off,
        EncryptionMode::Preferred => EncryptionLevel::On,
        EncryptionMode::Required => EncryptionLevel::Required,
    });
    if profile.trust_server_certificate {
        config.trust_cert();
    }

    let tcp = tokio::time::timeout(
        timeout,
        TcpStream::connect((profile.host.as_str(), profile.port)),
    )
    .await
    .map_err(|_| DomainError::Connection {
        message: format!(
            "connection to {}:{} timed out after {}s",
            profile.host,
            profile.port,
            timeout.as_secs()
        ),
    })?
    .map_err(|e| DomainError::Connection {
        message: format!(
            "could not connect to {}:{} — {e}",
            profile.host, profile.port
        ),
    })?;
    tcp.set_nodelay(true).ok();

    // Dropping the future on elapse drops the TCP stream — no partial
    // session escapes.
    let client = tokio::time::timeout(timeout, Client::connect(config, tcp.compat_write()))
        .await
        .map_err(|_| DomainError::Connection {
            message: format!(
                "login to {}:{} timed out after {}s",
                profile.host,
                profile.port,
                timeout.as_secs()
            ),
        })?
        .map_err(map_tiberius_error)?;
    Ok(Box::new(MssqlSession::new(client)))
}

/// Map a Tiberius error to a domain error. Auth error codes are detected by
/// inspecting [`tiberius::error::TokenError`]; TLS and I/O errors map to their
/// own variants so the service layer can surface actionable messages.
pub fn map_tiberius_error(e: tiberius::error::Error) -> DomainError {
    match e {
        tiberius::error::Error::Server(tok) => {
            classify_token(tok.code(), tok.message().to_owned(), tok.line())
        }
        tiberius::error::Error::Tls(m) => DomainError::Tls { message: m },
        tiberius::error::Error::Io { message, .. } => DomainError::Connection { message },
        other => DomainError::Driver {
            message: other.to_string(),
        },
    }
}

/// Classify a server error token: login-failure codes become
/// [`DomainError::Authentication`], everything else [`DomainError::Query`].
/// Separate from [`map_tiberius_error`] because `TokenError`'s fields are
/// private — constructing one in tests is not possible, but this function is.
fn classify_token(code: u32, message: String, line: u32) -> DomainError {
    if AUTH_ERROR_CODES.contains(&code) {
        DomainError::Authentication { message }
    } else {
        DomainError::Query {
            message,
            error_number: Some(code as i32),
            // The server reports line 0 when the number is not applicable.
            line: (line != 0).then_some(line),
            column: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_auth_error_codes() {
        for code in [18456u32, 18452, 18453] {
            let e = classify_token(code, "Login failed for user 'sa'.".into(), 1);
            assert!(
                matches!(e, DomainError::Authentication { .. }),
                "code {code}"
            );
        }
    }

    #[test]
    fn maps_other_server_errors_to_query() {
        let e = classify_token(208, "Invalid object name 'x'.".into(), 2);
        match e {
            DomainError::Query {
                message,
                error_number,
                line,
                column,
            } => {
                assert_eq!(message, "Invalid object name 'x'.");
                assert_eq!(error_number, Some(208));
                assert_eq!(line, Some(2));
                assert_eq!(column, None);
            }
            other => panic!("expected Query, got {other:?}"),
        }
    }
    /// Prelogin stall: a listener that accepts the TCP connection but never
    /// answers the handshake. `connect_inner` must time out (not hang) and
    /// surface a Connection error — before the fix only TcpStream::connect
    /// was bounded.
    #[tokio::test]
    async fn handshake_stall_times_out() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept and hold — never write a prelogin response.
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                std::mem::forget(s);
            }
        });

        let profile = ConnectionProfile {
            id: datara_domain::ConnectionId(0),
            name: "stall".into(),
            host: "127.0.0.1".into(),
            port,
            database: None,
            username: "sa".into(),
            authentication: datara_domain::AuthenticationMode::SqlPassword,
            encryption: EncryptionMode::Disabled,
            trust_server_certificate: true,
            secret_reference: datara_domain::SecretReference("t".into()),
        };
        let creds = Credentials {
            username: "sa".into(),
            password: secrecy::SecretString::from("x".to_string()),
        };
        let started = std::time::Instant::now();
        let err = connect_inner(&profile, &creds, Duration::from_millis(200))
            .await
            .err()
            .expect("stalled handshake must fail");
        assert!(
            matches!(err, DomainError::Connection { .. }),
            "expected Connection, got {err:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timeout didn't fire: {:?}",
            started.elapsed()
        );
    }
}
