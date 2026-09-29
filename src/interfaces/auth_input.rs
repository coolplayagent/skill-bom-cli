use super::Output;
use crate::{
    auth::Secret,
    domain::{Error, Result, auth::AuthStatus},
};
use std::io::{BufRead, IsTerminal, Write};

pub fn login_input(username: Option<&str>, password_stdin: bool) -> Result<(String, Secret)> {
    let terminal = std::io::stdin().is_terminal();
    if !terminal && (username.is_none() || !password_stdin) {
        return Err(input_error(
            "Noninteractive login requires --username and --password-stdin",
        ));
    }
    let username = match username {
        Some(name) => name.to_string(),
        None => {
            eprint!("W3 username: ");
            std::io::stderr().flush()?;
            read_line(std::io::stdin().lock(), 256)?
        }
    };
    let password = if password_stdin {
        if terminal {
            return Err(input_error(
                "Pipe the password into --password-stdin, or omit that flag for hidden input",
            ));
        }
        read_line(std::io::stdin().lock(), 4096)?
    } else {
        rpassword::prompt_password("W3 password: ")
            .map_err(|_| input_error("Cannot read hidden password input"))?
    };
    if username.trim().is_empty()
        || username.len() > 256
        || username.chars().any(char::is_control)
        || password.is_empty()
        || password.len() > 4096
    {
        return Err(input_error(
            "W3 username and password must be nonempty and within the input limits",
        ));
    }
    Ok((username, Secret::new(password)))
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
    let text = match (&status.username, status.expires_at, status.expiry_source) {
        (Some(username), Some(expires), Some(source)) => format!(
            "Local W3 login: {username}\nExpires: {} ({source:?})\nExpired: {}\nCleanup pending: {}",
            expires.to_rfc3339(),
            status.expired,
            status.cleanup_pending
        ),
        _ => format!(
            "No local W3 login.\nCleanup pending: {}",
            status.cleanup_pending
        ),
    };
    let mut output = Output::json(status)?;
    output.text = Some(text);
    Ok(output)
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
