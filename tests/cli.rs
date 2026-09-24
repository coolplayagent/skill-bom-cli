mod common;
use common::{
    http::{Reply, Server},
    *,
};
use skill_bom::domain::digest;
use std::path::Path;
use std::process::{Command, Output};
fn run(root: &Path, args: &[&str]) -> Output {
    let binary = Path::new(env!("CARGO_BIN_EXE_skill-bom"));
    let binary = if binary.is_absolute() {
        binary.to_path_buf()
    } else {
        std::env::current_dir().unwrap().join(binary)
    };
    Command::new(binary)
        .args(args)
        .current_dir(root)
        .env("SKILL_BOM_HOME", root.join("test-home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("test-home/cache"))
        .env("APPDATA", root.join("config"))
        .env("LOCALAPPDATA", root.join("data"))
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("ALL_PROXY", "http://127.0.0.1:1")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()
        .unwrap()
}
fn json(output: Output, code: i32) -> serde_json::Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    })
    .unwrap()
}
#[test]
fn complete_cli_archive_workflow_and_spdx_schema() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let archive = archive("example", "1.0.0");
    let hash = digest(&archive);
    let server = Server::new(move |_, _| Reply::bytes("application/zip", archive.clone()));
    write_manifest(root, &format!("{}/skill.zip", server.url), &hash);
    assert_eq!(
        json(run(root, &["validate", "--format", "json"]), 0)["valid"],
        true
    );
    assert_eq!(
        json(run(root, &["lock", "--format", "json"]), 0)["packages"],
        1
    );
    assert!(!root.join("skills").exists());
    let lock_bytes = std::fs::read(root.join("skills.lock")).unwrap();
    let plan = json(
        run(
            root,
            &["install", "--dry-run", "--frozen", "--format", "json"],
        ),
        0,
    );
    assert_eq!(plan["changes"][0]["action"], "add");
    assert!(!root.join("skills").exists());
    json(run(root, &["install", "--locked", "--format", "json"]), 0);
    assert!(root.join("skills/example/SKILL.md").exists());
    assert_eq!(std::fs::read(root.join("skills.lock")).unwrap(), lock_bytes);
    json(run(root, &["install", "--frozen", "--format", "json"]), 0);
    let tree = run(root, &["tree"]);
    assert!(tree.status.success());
    assert!(String::from_utf8_lossy(&tree.stdout).contains("example 1.0.0"));
    let paths = json(run(root, &["why", "example", "--format", "json"]), 0);
    assert_eq!(paths.as_array().unwrap().len(), 1);
    assert!(run(root, &["why", "missing"]).status.code() == Some(1));
    assert!(json(run(root, &["list", "--format", "json"]), 0)["installed"].is_object());
    let verify = json(run(root, &["verify", "--format", "json"]), 0);
    assert_eq!(verify["lock_differs"], false);
    let bom = json(run(root, &["bom", "--format", "json"]), 0);
    assert_eq!(bom["view"], "lock");
    assert_eq!(bom["generated_at"], "2023-11-14T22:13:20Z");
    assert_eq!(bom["remote_status_refreshed"], false);
    let again = json(run(root, &["bom", "--format", "json"]), 0);
    assert_eq!(bom, again);
    let spdx = json(
        run(
            root,
            &[
                "bom",
                "--format",
                "spdx-json",
                "--timestamp",
                "2026-01-01T00:00:00Z",
            ],
        ),
        0,
    );
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/spdx-2.3.schema.json")).unwrap();
    let validator = jsonschema::JSONSchema::compile(&schema).unwrap();
    assert!(validator.is_valid(&spdx), "{spdx}");
    skill_bom::bom::validate_references(&spdx).unwrap();
    std::fs::write(root.join("skills/example/SKILL.md"), "local edit").unwrap();
    json(run(root, &["verify", "--format", "json"]), 1);
    let drift = json(
        run(root, &["bom", "--from", "installed", "--format", "json"]),
        1,
    );
    assert_eq!(drift["view"], "installed");
    let output = run(root, &["install", "--frozen", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("INSTALL_CONFLICT"));
    let changed = std::fs::read_to_string(root.join("skills.toml"))
        .unwrap()
        .replace("name='test'", "name='changed'");
    std::fs::write(root.join("skills.toml"), changed).unwrap();
    assert_eq!(run(root, &["install", "--locked"]).status.code(), Some(1));
}
#[test]
fn init_update_scope_flags_errors_and_schema_exports() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    json(run(root, &["init", "--format", "json"]), 0);
    assert_eq!(run(root, &["init"]).status.code(), Some(1));
    assert_eq!(run(root, &["install", "--locked"]).status.code(), Some(1));
    assert_eq!(run(root, &["install", "--frozen"]).status.code(), Some(1));
    assert_eq!(
        run(root, &["validate", "--format", "spdx-json"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(run(root, &["verify"]).status.code(), Some(2));
    json(run(root, &["install", "--format", "json"]), 0);
    json(run(root, &["update", "--format", "json"]), 0);
    assert_eq!(run(root, &["update", "missing"]).status.code(), Some(2));
    json(run(root, &["init", "--global", "--format", "json"]), 0);
    json(run(root, &["install", "--global", "--format", "json"]), 0);
    let override_target = root.join("override");
    json(
        run(
            root,
            &[
                "install",
                "--target",
                override_target.to_str().unwrap(),
                "--format",
                "json",
            ],
        ),
        0,
    );
    assert_eq!(
        run(
            root,
            &[
                "install",
                "--global",
                "--target",
                override_target.to_str().unwrap()
            ]
        )
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        run(root, &["bom", "--timestamp", "bad"]).status.code(),
        Some(2)
    );
    for kind in ["manifest", "package", "lock", "bom", "installed", "error"] {
        let value = json(run(root, &["schema", kind, "--format", "json"]), 0);
        let checked: serde_json::Value = serde_json::from_slice(
            &std::fs::read(repository_root().join(format!("schemas/{kind}.schema.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(value, checked, "{kind}");
    }
    assert!(run(root, &["--help"]).status.success());
    assert_eq!(run(root, &["not-a-command"]).status.code(), Some(2));
    assert!(run(root, &["--version"]).status.success());
}
#[test]
fn locked_offline_missing_cache_and_strict_legacy_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let archive = zip(&[("SKILL.md", b"---\nname: legacy\n---\noriginal")]);
    let hash = digest(&archive);
    let server = Server::new(move |_, _| Reply::bytes("application/zip", archive.clone()));
    write_manifest(root, &format!("{}/legacy.zip", server.url), &hash);
    assert_eq!(run(root, &["install", "--offline"]).status.code(), Some(2));
    json(run(root, &["install", "--format", "json"]), 0);
    assert_eq!(
        run(root, &["install", "--strict-metadata"]).status.code(),
        Some(1)
    );
    assert_eq!(
        run(root, &["bom", "--strict-metadata"]).status.code(),
        Some(1)
    );
    let bom = json(run(root, &["bom", "--format", "json"]), 0);
    let p = bom["packages"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(p["metadata"]["dependency_metadata"], "unknown");
    json(run(root, &["update", "example", "--format", "json"]), 0);
    let config = std::fs::read_to_string(root.join("skills.toml")).unwrap();
    std::fs::write(root.join("skills.toml"),format!("{config}\n[[package_metadata]]\nname='legacy'\ncomplete=true\n[package_metadata.source]\nurl={:?}\nversion='=1.0.0'\nsha256={hash:?}\n",format!("{}/legacy.zip",server.url))).unwrap();
    json(
        run(root, &["lock", "--strict-metadata", "--format", "json"]),
        0,
    );
    json(
        run(
            root,
            &[
                "install",
                "--frozen",
                "--strict-metadata",
                "--format",
                "json",
            ],
        ),
        0,
    );
    // Remove the isolated child-process cache while preserving installed content.
    #[cfg(target_os = "linux")]
    {
        std::fs::remove_dir_all(root.join("test-home/cache")).unwrap();
        assert_eq!(run(root, &["install", "--frozen"]).status.code(), Some(2));
    }
}

#[test]
fn json_input_errors_and_invalid_generation_time_are_structured() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let bad = json(run(root, &["bad-command", "--format=json"]), 2);
    assert_eq!(bad["error"]["code"], "INPUT");
    let binary = Path::new(env!("CARGO_BIN_EXE_skill-bom"));
    let binary = if binary.is_absolute() {
        binary.to_path_buf()
    } else {
        std::env::current_dir().unwrap().join(binary)
    };
    let output = Command::new(&binary)
        .args(["init", "--format", "json"])
        .env("SKILL_BOM_HOME", "relative")
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(json(output, 2)["error"]["code"], "USER_DIRECTORY");
    json(run(root, &["init", "--format", "json"]), 0);
    json(run(root, &["lock", "--format", "json"]), 0);
    for epoch in ["invalid", "9999999999999999999", "9223372036854775807"] {
        let output = Command::new(&binary)
            .args(["bom", "--format", "json"])
            .env("SKILL_BOM_HOME", root.join("test-home"))
            .env("SOURCE_DATE_EPOCH", epoch)
            .current_dir(root)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("TIMESTAMP"));
    }
    let output = Command::new(&binary)
        .args(["bom", "--format", "json"])
        .env("SKILL_BOM_HOME", root.join("test-home"))
        .env_remove("SOURCE_DATE_EPOCH")
        .current_dir(root)
        .output()
        .unwrap();
    let value = json(output, 0);
    assert!(value["generated_at"].as_str().unwrap().ends_with('Z'));
}
