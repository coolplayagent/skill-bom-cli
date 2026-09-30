mod common;
use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn command(root: &Path, args: &[&str]) -> Command {
    let binary = Path::new(env!("CARGO_BIN_EXE_skill-bom"));
    let binary = if binary.is_absolute() {
        binary.to_path_buf()
    } else {
        std::env::current_dir().unwrap().join(binary)
    };
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .args(args)
        .env("SKILL_BOM_HOME", root.join("home"));
    command
}
fn run(root: &Path, args: &[&str], input: &str) -> Output {
    let mut child = command(root, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child.wait_with_output().unwrap()
}
fn error(output: &Output) -> Value {
    assert!(!output.status.success());
    serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"].clone()
}

#[test]
fn status_and_logout_work_before_project_config_and_support_text_json() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("skills.toml"), "invalid TOML").unwrap();
    for args in [
        vec!["auth", "status", "--format", "json"],
        vec![
            "--manifest",
            "missing.toml",
            "auth",
            "status",
            "--format",
            "json",
        ],
    ] {
        let output = run(root, &args, "");
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stderr.is_empty());
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status["logged_in"], false);
        assert_eq!(status["expires_at"], Value::Null);
    }
    assert!(!root.join("home/config/auth-v1").exists());
    let output = run(root, &["auth", "status"], "");
    assert!(String::from_utf8_lossy(&output.stdout).contains("No local W3 login"));
    for _ in 0..2 {
        let output = run(
            root,
            &["auth", "logout", "--offline", "--format", "json"],
            "",
        );
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["logged_out"],
            true
        );
    }
    let output = run(root, &["auth", "logout"], "");
    assert!(String::from_utf8_lossy(&output.stdout).contains("cleared"));
    assert_eq!(
        error(&run(root, &["auth", "status", "--format", "spdx-json"], ""))["code"],
        "FORMAT"
    );
}

#[test]
fn password_stdin_requirements_limits_and_offline_are_enforced_without_leaks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for args in [
        vec!["auth", "login", "--format", "json"],
        vec!["auth", "login", "--username", "alice", "--format", "json"],
        vec!["auth", "login", "--password-stdin", "--format", "json"],
    ] {
        let output = run(root, &args, "fixture-password\n");
        assert_eq!(error(&output)["code"], "AUTH_INPUT");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-password"));
    }
    for password in ["".into(), "fixture-password".repeat(300)] {
        let output = run(
            root,
            &[
                "auth",
                "login",
                "--username",
                "alice",
                "--password-stdin",
                "--format",
                "json",
            ],
            &password,
        );
        assert_eq!(error(&output)["code"], "AUTH_INPUT");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-password"));
    }
    let output = run(
        root,
        &["auth", "login", "--offline", "--format", "json"],
        "fixture-password\n",
    );
    assert_eq!(error(&output)["code"], "OFFLINE_MISS");
    let output = run(root, &["auth", "login", "--help"], "");
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--password-stdin") && help.contains("--username"));
    assert!(!help.contains("--password <") && !help.contains("PASSWORD_ENV"));
}

#[test]
fn local_status_does_not_validate_credentials_or_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let directory = root.join("home/config/auth-v1");
    std::fs::create_dir_all(&directory).unwrap();
    let state = serde_json::json!({"version":1,"session":{
        "username":"alice", "issued_at":"2020-01-01T00:00:00Z", "expires_at":"2020-01-01T04:00:00Z",
        "expiry_source":"local_policy", "credential_ref":"00000000-0000-4000-8000-000000000001",
        "login_id":"00000000-0000-4000-8000-000000000002"
    },"cleanup":[]});
    std::fs::write(
        directory.join("session.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let output = run(root, &["auth", "status", "--format", "json"], "");
    assert!(output.status.success());
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["logged_in"], true);
    assert_eq!(status["expired"], true);
    assert_eq!(status["expiry_source"], "local_policy");
    let output = run(root, &["auth", "status"], "");
    assert!(String::from_utf8_lossy(&output.stdout).contains("alice"));
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(directory.join("session.json")).unwrap())
            .unwrap(),
        state
    );
    #[cfg(target_os = "linux")]
    {
        // A deliberately nonexistent bus isolates this failure from any real keyring.
        let output = command(root, &["auth", "logout", "--format", "json"])
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", root.join("no-bus").display()),
            )
            .output()
            .unwrap();
        assert_eq!(error(&output)["code"], "AUTH_STORE_UNAVAILABLE");
        assert!(output.stdout.is_empty());
        let status = run(root, &["auth", "status", "--format", "json"], "");
        let status: Value = serde_json::from_slice(&status.stdout).unwrap();
        assert_eq!(status["logged_in"], false);
        assert_eq!(status["cleanup_pending"], true);
    }
}

#[test]
fn validate_accepts_no_token_env_and_ignores_broken_auth_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("skills.toml"), "schema_version=1\n[project]\nname='test'\n[registries.w3]\nkind='agentcenter'\nurl='https://agent.huawei.com'\n").unwrap();
    std::fs::create_dir_all(root.join("home/config/auth-v1")).unwrap();
    std::fs::write(root.join("home/config/auth-v1/session.json"), "broken").unwrap();
    let output = run(root, &["validate", "--format", "json"], "");
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["valid"],
        true
    );
}

#[test]
fn login_rejects_invalid_tls_setting_before_network_or_credential_storage() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = command(
        temp.path(),
        &[
            "auth",
            "login",
            "--username",
            "alice",
            "--password-stdin",
            "--format",
            "json",
        ],
    )
    .env("AGENTCENTER_VERIFY_TLS", "invalid-setting-secret")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture-password\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error(&output)["code"], "CONFIG");
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("invalid-setting-secret") && !text.contains("fixture-password"));
    assert!(
        !temp
            .path()
            .join("home/config/auth-v1/session.json")
            .exists()
    );
}

#[test]
fn origin_flags_token_input_and_offline_storage_do_not_leak_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let origin = "https://registry.example";
    for args in [
        vec!["auth", "login", "--origin", origin, "--username", "bob"],
        vec!["auth", "login", "--origin", origin, "--token-stdin"],
        vec![
            "auth",
            "login",
            "--origin",
            origin,
            "--username",
            "bob",
            "--password-stdin",
        ],
        vec!["auth", "login", "--username", "alice", "--token-stdin"],
    ] {
        let output = run(
            root,
            &args
                .into_iter()
                .chain(["--format", "json"])
                .collect::<Vec<_>>(),
            "private-token\n",
        );
        assert_eq!(error(&output)["code"], "AUTH_INPUT");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-token"));
    }
    for input in ["".into(), "bad\rheader\n".into(), "x".repeat(16385)] {
        let output = run(
            root,
            &[
                "auth",
                "login",
                "--origin",
                origin,
                "--username",
                "bob",
                "--token-stdin",
                "--format",
                "json",
            ],
            &input,
        );
        assert_eq!(error(&output)["code"], "AUTH_INPUT");
    }
    for subcommand in ["login", "status", "logout"] {
        let output = run(
            root,
            &[
                "auth",
                subcommand,
                "--origin",
                "https://user:private-token@example.com",
                "--format",
                "json",
            ],
            "",
        );
        assert_eq!(error(&output)["code"], "URL");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-token"));
    }
    assert!(
        !run(root, &["auth", "status", "--all", "--origin", origin], "")
            .status
            .success()
    );
    assert!(
        !run(
            root,
            &["auth", "login", "--password-stdin", "--token-stdin"],
            ""
        )
        .status
        .success()
    );
    assert!(!root.join("home/config/auth-v1").exists());
    #[cfg(target_os = "linux")]
    {
        let mut child = command(
            root,
            &[
                "auth",
                "login",
                "--offline",
                "--origin",
                origin,
                "--username",
                "bob",
                "--token-stdin",
                "--format",
                "json",
            ],
        )
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", root.join("no-bus").display()),
        )
        .env("AGENTCENTER_VERIFY_TLS", "invalid-but-unused")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"private-token\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(error(&output)["code"], "AUTH_STORE_UNAVAILABLE");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-token"));
    }
}

#[test]
fn origin_status_is_local_sorted_and_logout_leaves_other_origins() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("skills.toml"), "invalid TOML").unwrap();
    let output = run(root, &["auth", "status", "--all", "--format", "json"], "");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["sessions"],
        serde_json::json!([])
    );
    assert!(
        String::from_utf8_lossy(&run(root, &["auth", "status", "--all"], "").stdout)
            .contains("No saved logins")
    );
    assert!(!root.join("home/config/auth-v1").exists());
    for origin in ["https://z.example", "https://a.example"] {
        let directory = root
            .join("home/config/auth-v1/origins")
            .join(skill_bom::domain::digest(origin.as_bytes()));
        std::fs::create_dir_all(&directory).unwrap();
        let state = serde_json::json!({"origin":origin,"version":1,"session":{
            "method":"token","username":"bob","issued_at":"2020-01-01T00:00:00Z",
            "expires_at":null,"expiry_source":null,
            "credential_ref":"00000000-0000-4000-8000-000000000001",
            "login_id":"00000000-0000-4000-8000-000000000002"
        },"cleanup":[]});
        std::fs::write(
            directory.join("session.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
    }
    let output = run(
        root,
        &["auth", "status", "--all", "--offline", "--format", "json"],
        "",
    );
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let origins = value["sessions"].as_array().unwrap();
    assert_eq!(origins.len(), 2);
    assert_eq!(origins[0]["origin"], "https://a.example");
    assert!(origins[0]["expired"].is_null());
    assert_eq!(origins[0]["method"], "token");
    let output = run(
        root,
        &["auth", "status", "--origin", "https://A.example:443/"],
        "",
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("not verified"));
    let output = run(root, &["auth", "status", "--all"], "");
    assert!(String::from_utf8_lossy(&output.stdout).contains("https://z.example"));
    // The missing default login can be logged out without touching any real keyring.
    assert!(run(root, &["auth", "logout"], "").status.success());
    let output = run(
        root,
        &[
            "auth",
            "status",
            "--origin",
            "https://a.example",
            "--format",
            "json",
        ],
        "",
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["logged_in"],
        true
    );
    let output = run(
        root,
        &["auth", "status", "--origin", "https://missing.example"],
        "",
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("No local token login"));
    assert!(
        run(
            root,
            &["auth", "logout", "--origin", "https://missing.example"],
            ""
        )
        .status
        .success()
    );
}
