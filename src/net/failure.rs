//! Typed, bounded error classification without exposing URLs or backend diagnostics.
use crate::domain::Error;
use std::error::Error as StdError;
use std::io;

pub(super) fn request_error(error: &(dyn StdError + 'static)) -> Error {
    let mut current = Some(error);
    let mut timeout = false;
    for _ in 0..16 {
        let Some(cause) = current else { break };
        if let Some(tls) = cause.downcast_ref::<rustls::Error>() {
            let message = if matches!(
                tls,
                rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented
            ) {
                "TLS certificate verification failed"
            } else {
                "TLS handshake or protocol failed"
            };
            return Error::new("NETWORK_TLS", message, 2).phase("download").hint(
                "Check the server certificate chain, hostname and TLS configuration; for W3/AgentCenter also review AGENTCENTER_VERIFY_TLS.",
            );
        }
        timeout |= cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout);
        current = if let Some(io) = cause.downcast_ref::<io::Error>() {
            timeout |= matches!(
                io.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            );
            // io::Error::source can skip its contained error's concrete type.
            io.get_ref().map(|inner| inner as &(dyn StdError + 'static))
        } else {
            cause.source()
        };
    }
    if timeout {
        Error::new("NETWORK_TIMEOUT", "HTTP request timed out", 2)
            .phase("download")
            .hint("Check connectivity and service availability, then retry.")
    } else {
        Error::new("NETWORK", "HTTP connection or response failed", 2)
            .phase("download")
            .hint("Check connectivity, proxy configuration and service availability, then retry.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_errors_are_distinct_and_never_echo_backend_messages() {
        for (cause, code, message) in [
            (
                io::Error::new(io::ErrorKind::TimedOut, "private-token"),
                "NETWORK_TIMEOUT",
                "HTTP request timed out",
            ),
            (
                io::Error::new(io::ErrorKind::ConnectionRefused, "private-token"),
                "NETWORK",
                "HTTP connection or response failed",
            ),
            (
                io::Error::other(rustls::Error::General("private-token".into())),
                "NETWORK_TLS",
                "TLS handshake or protocol failed",
            ),
            (
                io::Error::other(rustls::Error::NoCertificatesPresented),
                "NETWORK_TLS",
                "TLS certificate verification failed",
            ),
        ] {
            let error = request_error(&cause);
            assert_eq!(error.code, code);
            assert_eq!(error.message.as_ref(), message);
            assert_eq!(error.exit_code, 2);
            assert!(!format!("{error:?}").contains("private-token"));
        }
    }
}
