mod common;
use common::{
    http::{Reply, Server},
    *,
};
use skill_bom::{config, domain::*, store::Store};
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    let binary = Path::new(env!("CARGO_BIN_EXE_skill-bom"));
    let binary = if binary.is_absolute() {
        binary.to_owned()
    } else {
        std::env::current_dir().unwrap().join(binary)
    };
    Command::new(binary)
        .args(args)
        .current_dir(root)
        .env("SKILL_BOM_HOME", root.join("test-home"))
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
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.stdout.is_empty() {
        let stderr = String::from_utf8(output.stderr).unwrap();
        let error: serde_json::Value =
            serde_json::from_str(stderr.lines().last().unwrap()).unwrap();
        error["error"].clone()
    } else {
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
fn assert_target(value: &serde_json::Value, expected: &Path) {
    let actual = Path::new(value.as_str().unwrap());
    assert!(actual.is_absolute());
    // Resolve existing ancestors without creating a dry-run target. This also
    // recognizes macOS's trusted /var -> /private/var alias and Windows prefixes.
    let resolve = |path: &Path| {
        let ancestor = path.ancestors().find(|p| p.exists()).unwrap();
        ancestor
            .canonicalize()
            .unwrap()
            .join(path.strip_prefix(ancestor).unwrap())
    };
    assert_eq!(resolve(actual), resolve(expected));
}
const PRESETS: [(&str, &str, &str); 5] = [
    ("universal", ".agents/skills", ".agents/skills"),
    ("codex", ".agents/skills", ".agents/skills"),
    ("claude-code", ".claude/skills", ".claude/skills"),
    ("cursor", ".cursor/skills", ".cursor/skills"),
    ("relayagent", ".skills", ".relay/skills"),
];

#[test]
fn every_agent_discovers_standard_skills_in_both_isolated_scopes() {
    let entry = b"---\nname: review\ndescription: Review changes when asked\nlicense: MIT\nmetadata:\n  version: '1.0'\ndisable-model-invocation: true\n---\nRead references/guide.md. !`never execute`\n";
    let archive = zip(&[
        ("SKILL.md", entry),
        ("references/guide.md", b"reference"),
        ("scripts/run.sh", b"exit 99"),
        ("assets/template", b"asset"),
        (
            "agents/openai.yaml",
            b"interface:\n  display_name: Review\n",
        ),
    ]);
    let hash = digest(&archive);
    let server = Server::new(move |_, _| Reply::bytes("application/zip", archive.clone()));
    for (agent, project, global_path) in PRESETS {
        for global in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            let mut args = vec!["--agent", agent, "--format", "json"];
            if global {
                args.push("--global");
            }
            let mut init = args.clone();
            init.push("init");
            let created = json(run(root, &init), 0);
            let manifest = Path::new(created["created"].as_str().unwrap());
            let saved =
                config::Manifest::parse(&std::fs::read_to_string(manifest).unwrap()).unwrap();
            assert_eq!(saved.install.agent.unwrap().as_str(), agent);
            let target = if global {
                root.join("test-home/home").join(global_path)
            } else {
                root.join(project)
            };
            let saved_list = if global {
                vec!["list", "--global", "--format", "json"]
            } else {
                vec!["list", "--format", "json"]
            };
            assert_target(&json(run(root, &saved_list), 0)["target"], &target);
            write_manifest(
                manifest.parent().unwrap(),
                &format!("{}/review.zip", server.url),
                &hash,
            );
            let mut preview = args.clone();
            preview.extend(["install", "--dry-run"]);
            let plan = json(run(root, &preview), 0);
            assert_target(&plan["target"], &target);
            assert_eq!(plan["agent"], agent);
            assert!(!target.exists());
            assert!(!manifest.with_extension("lock").exists());
            let mut install = args.clone();
            install.push("install");
            json(run(root, &install), 0);
            // This is a filesystem discovery contract, not a live Agent invocation.
            let skill =
                config::skill::parse(&std::fs::read(target.join("review/SKILL.md")).unwrap())
                    .unwrap();
            assert_eq!(skill.name, "review");
            assert_eq!(skill.description, "Review changes when asked");
            assert_eq!(
                std::fs::read(target.join("review/SKILL.md")).unwrap(),
                entry
            );
            for (path, expected) in [
                ("references/guide.md", "reference"),
                ("scripts/run.sh", "exit 99"),
                ("assets/template", "asset"),
                ("agents/openai.yaml", "interface:\n  display_name: Review\n"),
            ] {
                assert_eq!(
                    std::fs::read(target.join("review").join(path)).unwrap(),
                    expected.as_bytes()
                );
            }
            let lock_before = std::fs::read(manifest.with_extension("lock")).unwrap();
            let mut frozen = args.clone();
            frozen.extend(["install", "--frozen"]);
            json(run(root, &frozen), 0);
            assert_eq!(
                std::fs::read(manifest.with_extension("lock")).unwrap(),
                lock_before
            );
            let mut verify = args.clone();
            verify.push("verify");
            json(run(root, &verify), 0);
            let mut list = args;
            list.push("list");
            assert_eq!(json(run(root, &list), 0)["agent"], agent);
        }
    }
}

#[test]
fn selection_precedence_conflicts_and_init_persistence() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    json(
        run(root, &["init", "--agent", "relayagent", "--format", "json"]),
        0,
    );
    assert_target(
        &json(run(root, &["install", "--format", "json"]), 0)["target"],
        &root.join(".skills"),
    );
    assert_target(
        &json(
            run(root, &["install", "--agent", "cursor", "--format", "json"]),
            0,
        )["target"],
        &root.join(".cursor/skills"),
    );
    let custom = json(
        run(root, &["install", "--target", "custom", "--format", "json"]),
        0,
    );
    assert_target(&custom["target"], &root.join("custom"));
    assert!(custom["agent"].is_null());
    assert_eq!(
        run(root, &["install", "--agent", "cursor", "--target", "x"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(root, &["install", "--agent", "unknown"]).status.code(),
        Some(2)
    );
    std::fs::write(
        root.join("skills.toml"),
        "schema_version=1\n[project]\nname='test'\n[install]\ntarget='old'\n",
    )
    .unwrap();
    assert_target(
        &json(run(root, &["list", "--format", "json"]), 0)["target"],
        &root.join("old"),
    );
    assert_target(
        &json(
            run(root, &["list", "--agent", "relayagent", "--format", "json"]),
            0,
        )["target"],
        &root.join(".skills"),
    );
    std::fs::write(
        root.join("skills.toml"),
        "schema_version=1\n[project]\nname='test'\n[install]\ntarget='old'\nagent='cursor'\n",
    )
    .unwrap();
    assert_eq!(
        json(run(root, &["validate", "--format", "json"]), 2)["code"],
        "CONFIG"
    );
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    json(
        run(&nested, &["init", "--target", "chosen", "--format", "json"]),
        0,
    );
    assert_target(
        &json(
            run(
                root,
                &[
                    "list",
                    "--manifest",
                    "nested/skills.toml",
                    "--format",
                    "json",
                ],
            ),
            0,
        )["target"],
        &nested.join("chosen"),
    );
}

#[test]
fn default_change_preserves_legacy_target_and_manifest_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let created = json(run(root, &["init", "--format", "json"]), 0);
    let manifest_path = Path::new(created["created"].as_str().unwrap());
    let m = config::Manifest::parse(&std::fs::read_to_string(manifest_path).unwrap()).unwrap();
    let serialized = serde_json::to_value(&m).unwrap();
    assert_eq!(serialized["install"], serde_json::json!({"target":null}));
    let original = br#"{"schema_version":1,"project":{"name":"my-project"},"install":{"target":null},"registries":{},"dependencies":{},"package_metadata":[]}"#;
    assert_eq!(m.digest().unwrap(), digest(original));
    json(
        run(root, &["install", "--target", "skills", "--format", "json"]),
        0,
    );
    let state_path = root.join("skills/.skill-bom/state.json");
    let before = std::fs::read(&state_path).unwrap();
    let result = run(root, &["install", "--format", "json"]);
    assert!(String::from_utf8_lossy(&result.stderr).contains("Legacy installation remains"));
    assert_target(&json(result, 0)["target"], &root.join(".agents/skills"));
    assert_eq!(std::fs::read(&state_path).unwrap(), before);
}

#[test]
fn invalid_skill_and_reserved_names_fail_before_target_or_lock_mutation() {
    for (entry, agent, expected) in [
        ("---\nname: review\n---\n", "universal", "SKILL_FORMAT"),
        (
            "---\nname: synced\ndescription: Test\n---\n",
            "claude-code",
            "AGENT_SKILL_NAME",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let archive = zip(&[("SKILL.md", entry.as_bytes())]);
        let hash = digest(&archive);
        let server = Server::new(move |_, _| Reply::bytes("application/zip", archive.clone()));
        write_manifest(root, &format!("{}/review.zip", server.url), &hash);
        assert_eq!(
            json(
                run(root, &["install", "--agent", agent, "--format", "json"]),
                1
            )["code"],
            expected
        );
        assert!(!root.join("skills.lock").exists());
        assert!(
            !root
                .join(agent.parse::<Agent>().unwrap().directory(false))
                .exists()
        );
    }
}

#[test]
fn old_locked_offline_cache_cannot_bypass_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    json(run(root, &["init", "--format", "json"]), 0);
    let cache = Store::new(root.join("test-home/cache/content-v1"));
    let mut p = package(&cache, "review", "1.0.0");
    let content = tempfile::tempdir().unwrap();
    std::fs::write(content.path().join("SKILL.md"), "---\nname: review\n---\n").unwrap();
    (p.tree_sha256, p.files) = cache.publish(content.path()).unwrap();
    let mut old = lock(vec![p]);
    old.manifest_digest =
        config::Manifest::parse(&std::fs::read_to_string(root.join("skills.toml")).unwrap())
            .unwrap()
            .digest()
            .unwrap();
    config::write_lock(&root.join("skills.lock"), &old).unwrap();
    let before = std::fs::read(root.join("skills.lock")).unwrap();
    assert_eq!(
        json(run(root, &["install", "--frozen", "--format", "json"]), 1)["code"],
        "SKILL_FORMAT"
    );
    assert_eq!(std::fs::read(root.join("skills.lock")).unwrap(), before);
    assert!(!root.join(".agents/skills").exists());
}
