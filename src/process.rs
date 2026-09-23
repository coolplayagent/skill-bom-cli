//! Git process ownership, bounded output, timeouts and cooperative interruption.
use crate::domain::*;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
pub fn install_interrupt_handler() -> Result<()> {
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .map_err(|_| Error::new("SIGNAL", "Cannot install interrupt handler", 2))
}
pub fn check_interrupt() -> Result<()> {
    if INTERRUPTED.load(Ordering::SeqCst) {
        return Err(Error::new("INTERRUPTED", "Operation interrupted", 130));
    }
    Ok(())
}
pub fn git(dir: Option<&Path>, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.args([
        "-c",
        "core.hooksPath=",
        "-c",
        "credential.interactive=false",
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.file.allow=always",
    ]);
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command.spawn().map_err(|_| {
        Error::new("GIT_UNAVAILABLE", "Cannot start Git", 2)
            .hint("Install Git and ensure it is available on PATH.")
    })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || drain(stdout, MAX_BYTES));
    let err = std::thread::spawn(move || drain(stderr, 65536));
    let start = Instant::now();
    let mut failure = None;
    let status = loop {
        if let Err(e) = check_interrupt() {
            failure = Some(e);
            terminate(&mut child);
        }
        if start.elapsed() > Duration::from_secs(90) {
            failure = Some(Error::new(
                "GIT_TIMEOUT",
                "Git exceeded 90-second budget",
                2,
            ));
            terminate(&mut child);
        }
        if let Some(s) = child.try_wait()? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let bytes = out
        .join()
        .map_err(|_| Error::new("PROCESS", "Git output worker failed", 2))??;
    err.join()
        .map_err(|_| Error::new("PROCESS", "Git error worker failed", 2))??;
    if let Some(e) = failure {
        return Err(e);
    }
    if !status.success() {
        return Err(Error::new(
            "GIT_FAILED",
            format!("Git command failed with status {status}"),
            2,
        )
        .hint("Check repository access, revision and Git credentials."));
    }
    Ok(bytes)
}
fn drain(mut input: impl Read, max: u64) -> Result<Vec<u8>> {
    let mut out = vec![];
    let mut buf = [0u8; 8192];
    let mut overflow = false;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if out.len() as u64 + n as u64 > max {
            overflow = true;
        } else if !overflow {
            out.extend_from_slice(&buf[..n]);
        }
    }
    if overflow {
        return Err(Error::new("RESOURCE_LIMIT", "Git output exceeds budget", 2));
    }
    Ok(out)
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(child.id() as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    #[cfg(windows)]
    {
        if let Ok(mut killer) = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(2) {
                if killer.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = killer.kill();
            let _ = killer.wait();
        }
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_output_reports_overflow_without_truncating_success() {
        assert_eq!(drain(&b"abc"[..], 3).unwrap(), b"abc");
        assert_eq!(drain(&b"abcdef"[..], 3).unwrap_err().code, "RESOURCE_LIMIT");
        check_interrupt().unwrap();
    }
}
