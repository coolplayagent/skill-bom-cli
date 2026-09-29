//! Every credential here is a fixture in a temporary directory, never an OS keyring.
#[path = "common/auth.rs"]
mod fixture;
use fixture::*;
use skill_bom::{
    auth::*,
    domain::{Result, auth::ExpirySource},
};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

struct DiskSecrets(PathBuf);
impl DiskSecrets {
    fn path(&self, namespace: &str, reference: &str) -> PathBuf {
        self.0.join(format!(
            "{}-{reference}",
            skill_bom::domain::digest(namespace.as_bytes())
        ))
    }
}
impl SecretStore for DiskSecrets {
    fn get(&self, namespace: &str, reference: &str) -> Result<Option<Secret>> {
        match std::fs::read_to_string(self.path(namespace, reference)) {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn set(&self, namespace: &str, reference: &str, secret: &Secret) -> Result<()> {
        std::fs::create_dir_all(&self.0)?;
        std::fs::write(self.path(namespace, reference), secret.expose())?;
        Ok(())
    }
    fn delete(&self, namespace: &str, reference: &str) -> Result<()> {
        match std::fs::remove_file(self.path(namespace, reference)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
struct CountingLogin(PathBuf);
impl LoginGateway for CountingLogin {
    fn login(
        &self,
        _: &str,
        password: &Secret,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<LoginResult> {
        assert_eq!(password.expose(), "fixture-password");
        let n: usize = std::fs::read_to_string(&self.0)
            .unwrap_or_else(|_| "0".into())
            .parse()
            .unwrap();
        std::fs::write(&self.0, (n + 1).to_string())?;
        Ok(LoginResult {
            token: Secret::new(format!("token-{}", n + 1)),
            expires_at: now + chrono::Duration::hours(4),
            expiry_source: ExpirySource::LocalPolicy,
        })
    }
}
struct InterruptedSecrets {
    disk: DiskSecrets,
    after: String,
}
impl SecretStore for InterruptedSecrets {
    fn get(&self, namespace: &str, reference: &str) -> Result<Option<Secret>> {
        self.disk.get(namespace, reference)
    }
    fn set(&self, namespace: &str, reference: &str, secret: &Secret) -> Result<()> {
        self.disk.set(namespace, reference, secret)?;
        if reference.ends_with(&self.after) {
            // Exit without unwinding while publication still holds the process lock.
            std::process::exit(17);
        }
        Ok(())
    }
    fn delete(&self, namespace: &str, reference: &str) -> Result<()> {
        self.disk.delete(namespace, reference)
    }
}
fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "fixture signal timed out: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn worker(root: &Path, mode: &str, name: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "auth_process_worker", "--nocapture"])
        .env("SKILL_BOM_AUTH_TEST_ROOT", root)
        .env("SKILL_BOM_AUTH_TEST_MODE", mode)
        .env("SKILL_BOM_AUTH_TEST_NAME", name)
        .spawn()
        .unwrap()
}
fn join(mut child: Child) {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("auth worker timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn auth_process_worker() {
    let Ok(root) = std::env::var("SKILL_BOM_AUTH_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let mode = std::env::var("SKILL_BOM_AUTH_TEST_MODE").unwrap();
    let name = std::env::var("SKILL_BOM_AUTH_TEST_NAME").unwrap();
    let sessions = Sessions::new(&root.join("config")).unwrap();
    if mode == "interrupt_publication" {
        let secrets = InterruptedSecrets {
            disk: DiskSecrets(root.join("fixture-secrets")),
            after: name,
        };
        sessions
            .service(
                &secrets,
                &CountingLogin(root.join("logins")),
                &FakeClock::default(),
            )
            .login("bob", &password())
            .unwrap();
        panic!("publication did not reach the interruption point");
    }
    if mode == "hold_lock" {
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("config/auth-v1/session.lock"))
            .unwrap();
        fs2::FileExt::lock_exclusive(&lock).unwrap();
        std::fs::write(root.join(format!("ready-{name}")), "").unwrap();
        wait_for(&root.join("release"));
        return;
    }
    let secrets = DiskSecrets(root.join("fixture-secrets"));
    let gateway = CountingLogin(root.join("logins"));
    let clock = FakeClock::default();
    let service = sessions.service(&secrets, &gateway, &clock);
    let stale = service.acquire(ORIGIN, None).unwrap();
    std::fs::write(root.join(format!("ready-{name}")), "").unwrap();
    wait_for(&root.join("release"));
    match mode.as_str() {
        "refresh" => assert_eq!(
            service.refresh(ORIGIN, &stale).unwrap().token.expose(),
            "token-2"
        ),
        "after_logout" => assert_eq!(
            service.refresh(ORIGIN, &stale).unwrap_err().code,
            "AUTH_REQUIRED"
        ),
        _ => panic!("unknown fixture mode"),
    }
}
fn seed(root: &Path) {
    Sessions::new(&root.join("config"))
        .unwrap()
        .service(
            &DiskSecrets(root.join("fixture-secrets")),
            &CountingLogin(root.join("logins")),
            &FakeClock::default(),
        )
        .login("alice", &password())
        .unwrap();
}

#[test]
fn processes_merge_refresh_and_logout_wins_over_waiting_snapshot() {
    for mode in ["refresh", "after_logout"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        seed(root);
        let first = worker(root, mode, "first");
        let second = worker(root, mode, "second");
        wait_for(&root.join("ready-first"));
        wait_for(&root.join("ready-second"));
        if mode == "after_logout" {
            Sessions::new(&root.join("config"))
                .unwrap()
                .logout(&DiskSecrets(root.join("fixture-secrets")))
                .unwrap();
        }
        std::fs::write(root.join("release"), "").unwrap();
        join(first);
        join(second);
        assert_eq!(
            std::fs::read_to_string(root.join("logins")).unwrap(),
            if mode == "refresh" { "2" } else { "1" }
        );
        assert_eq!(
            Sessions::new(&root.join("config"))
                .unwrap()
                .status(&FakeClock::default())
                .unwrap()
                .logged_in,
            mode == "refresh"
        );
    }
}

#[test]
fn authentication_lock_wait_is_bounded_and_released_by_process_exit() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    seed(root);
    let child = worker(root, "hold_lock", "lock");
    wait_for(&root.join("ready-lock"));
    let sessions = Sessions::new(&root.join("config"))
        .unwrap()
        .with_lock_timeout(Duration::from_millis(40));
    let start = Instant::now();
    assert_eq!(
        sessions
            .logout(&DiskSecrets(root.join("fixture-secrets")))
            .unwrap_err()
            .code,
        "AUTH_STORE_UNAVAILABLE"
    );
    assert!(start.elapsed() >= Duration::from_millis(40));
    assert!(start.elapsed() < Duration::from_secs(2));
    std::fs::write(root.join("release"), "").unwrap();
    join(child);
    sessions
        .logout(&DiskSecrets(root.join("fixture-secrets")))
        .unwrap();
}

#[test]
fn interrupted_secret_publication_keeps_previous_login_and_cleanup_references() {
    for after in [".password", ".token"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        seed(root);
        let mut child = worker(root, "interrupt_publication", after);
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(15) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("interrupted publication worker timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(17));
        let sessions = Sessions::new(&root.join("config")).unwrap();
        let secrets = DiskSecrets(root.join("fixture-secrets"));
        let clock = FakeClock::default();
        let status = sessions.status(&clock).unwrap();
        assert_eq!(status.username.as_deref(), Some("alice"));
        assert!(status.cleanup_pending);
        assert_eq!(
            sessions
                .service(&secrets, &CountingLogin(root.join("logins")), &clock)
                .acquire(ORIGIN, None)
                .unwrap()
                .token
                .expose(),
            "token-1"
        );
        assert_eq!(
            std::fs::read_dir(&secrets.0).unwrap().count(),
            if after == ".password" { 3 } else { 4 }
        );
        sessions.logout(&secrets).unwrap();
        assert_eq!(std::fs::read_dir(&secrets.0).unwrap().count(), 0);
        let status = sessions.status(&clock).unwrap();
        assert!(!status.logged_in && !status.cleanup_pending);
    }
}
