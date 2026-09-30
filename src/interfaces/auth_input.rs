use super::Output;
use crate::{
    auth::Secret,
    domain::{
        Error, Result,
        auth::{AuthMethod, AuthStatus},
    },
};
use std::io::{BufRead, IsTerminal, Write};

pub fn login_input(username: Option<&str>, password_stdin: bool) -> Result<(String, Secret)> {
    credential_input(username, password_stdin, AuthMethod::W3)
}
pub fn token_input(username: Option<&str>, token_stdin: bool) -> Result<(String, Secret)> {
    credential_input(username, token_stdin, AuthMethod::Token)
}
fn credential_input(
    username: Option<&str>,
    from_stdin: bool,
    method: AuthMethod,
) -> Result<(String, Secret)> {
    let (flag, prompt, limit) = match method {
        AuthMethod::W3 => ("--password-stdin", "W3 password: ", 4096),
        AuthMethod::Token => ("--token-stdin", "Token: ", 16384),
    };
    let terminal = std::io::stdin().is_terminal();
    if !terminal && (username.is_none() || !from_stdin) {
        return Err(input_error(&format!(
            "Noninteractive login requires --username and {flag}"
        )));
    }
    let username = match username {
        Some(name) => name.to_string(),
        None => {
            eprint!("Account username: ");
            std::io::stderr().flush()?;
            read_line(std::io::stdin().lock(), 256)?
        }
    };
    let secret = Secret::new(if from_stdin {
        if terminal {
            return Err(input_error(&format!(
                "Pipe the secret into {flag}, or omit that flag for hidden input"
            )));
        }
        read_line(std::io::stdin().lock(), limit)?
    } else {
        rpassword::prompt_password(prompt)
            .map_err(|_| input_error("Cannot read hidden credential input"))?
    });
    if username.trim().is_empty()
        || username.len() > 256
        || username.chars().any(char::is_control)
        || secret.expose().is_empty()
        || secret.expose().len() > limit
        || (method == AuthMethod::Token && !crate::auth::valid_token(secret.expose()))
    {
        return Err(input_error(
            "Username and credential must be nonempty and within the input limits",
        ));
    }
    Ok((username, secret))
}
fn input_error(message: &str) -> Error {
    Error::new("AUTH_INPUT", message, 2).phase("authentication")
}
fn read_line(reader: impl BufRead, limit: usize) -> Result<String> {
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    // Take only one bounded line; do not wait for EOF on an interactive username.
    reader
        .take((limit + 2) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| input_error("Cannot read authentication input"))?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.len() > limit {
        return Err(input_error("Authentication input exceeds its size limit"));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| input_error("Authentication input must be UTF-8"))
}
pub fn render_status(status: AuthStatus) -> Result<Output> {
    let text = status_text(&status);
    let mut output = Output::json(status)?;
    output.text = Some(text);
    Ok(output)
}
pub fn render_statuses(statuses: Vec<AuthStatus>) -> Result<Output> {
    let text = if statuses.is_empty() {
        "No saved logins.".into()
    } else {
        statuses
            .iter()
            .map(status_text)
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let mut output = Output::json(serde_json::json!({"sessions": statuses}))?;
    output.text = Some(text);
    Ok(output)
}
fn status_text(status: &AuthStatus) -> String {
    let detail = match (&status.username, status.expires_at, status.expiry_source) {
        (Some(username), Some(expires), Some(source)) => format!(
            "Local W3 login: {username}\nExpires: {} ({source:?})\nExpired: {}\nCleanup pending: {}",
            expires.to_rfc3339(),
            status.expired.unwrap_or(false),
            status.cleanup_pending
        ),
        (Some(username), _, _) if status.method == AuthMethod::Token => format!(
            "Local token saved for: {username}\nServer validity: not verified\nExpiry: unknown\nCleanup pending: {}",
            status.cleanup_pending
        ),
        _ if status.method == AuthMethod::W3 => format!(
            "No local W3 login.\nCleanup pending: {}",
            status.cleanup_pending
        ),
        _ => format!(
            "No local token login.\nCleanup pending: {}",
            status.cleanup_pending
        ),
    };
    format!("Origin: {}\n{detail}", status.origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_lines_preserve_password_spaces_and_remove_only_line_endings() {
        assert_eq!(read_line(&b" secret \r\n"[..], 8).unwrap(), " secret ");
        assert_eq!(read_line(&b"bob\nignored"[..], 3).unwrap(), "bob");
        assert!(read_line(&b"12345"[..], 4).is_err());
        assert!(read_line(&b"\xff\n"[..], 4).is_err());
    }
}
