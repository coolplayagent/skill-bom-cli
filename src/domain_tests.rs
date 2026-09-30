use super::*;

#[test]
fn agent_skills_names_are_unicode_portable_and_canonical() {
    for name in ["review", "代码审查", "étude", "a-1", "４２"] {
        assert!(skill_name(name).is_ok(), "{name}");
    }
    assert_eq!(skill_name(" e\u{301}tude ").unwrap(), "étude");
    assert_eq!(skill_name("４２").unwrap(), "42");
    for name in [
        "", "Review", "foo_bar", "a.b", "a--b", "-a", "a-", "a/b", "CON", "nul",
    ] {
        assert!(skill_name(name).is_err(), "{name}");
    }
    assert!(skill_name(&"文".repeat(64)).is_ok());
    assert!(skill_name(&"文".repeat(65)).is_err());
    assert_eq!(directory_key("ＥXAMPLE"), "example");
}

#[test]
fn agent_presets_have_explicit_project_and_user_contracts() {
    for (name, agent, project, global) in [
        (
            "universal",
            Agent::Universal,
            ".agents/skills",
            ".agents/skills",
        ),
        ("codex", Agent::Codex, ".agents/skills", ".agents/skills"),
        (
            "claude-code",
            Agent::ClaudeCode,
            ".claude/skills",
            ".claude/skills",
        ),
        ("cursor", Agent::Cursor, ".cursor/skills", ".cursor/skills"),
        ("relayagent", Agent::Relayagent, ".skills", ".relay/skills"),
    ] {
        assert_eq!(name.parse::<Agent>().unwrap(), agent);
        assert_eq!(agent.as_str(), name);
        assert_eq!(agent.directory(false), project);
        assert_eq!(agent.directory(true), global);
        agent.validate_name("review").unwrap();
    }
    assert!("unknown".parse::<Agent>().is_err());
    for name in ["synced", "anthropic-skills"] {
        assert!(Agent::ClaudeCode.validate_name(name).is_err());
        Agent::Universal.validate_name(name).unwrap();
    }
}

#[test]
fn auth_expiry_and_refresh_are_pure() {
    use auth::{ExpirySource, Session, expiry};
    let now = chrono::DateTime::from_timestamp(1_790_640_000, 0).unwrap();
    let (expires_at, expiry_source) = expiry(&serde_json::json!({}), now);
    assert_eq!(expires_at, now + chrono::Duration::hours(4));
    assert_eq!(expiry_source, ExpirySource::LocalPolicy);
    let session = Session {
        username: "alice".into(),
        issued_at: now,
        expires_at,
        expiry_source,
        credential_ref: "reference".into(),
        login_id: "login".into(),
    };
    assert!(!session.refresh_due(now + chrono::Duration::seconds(7199)));
    assert!(session.refresh_due(now + chrono::Duration::hours(2)));
    assert!(session.refresh_due(now - chrono::Duration::seconds(1)));
    let (expires_at, source) = expiry(
        &serde_json::json!({"cloudDragonTokens":{"expiresIn":3600},"expiresAt":(now + chrono::Duration::seconds(20)).to_rfc3339()}),
        now,
    );
    assert_eq!(source, ExpirySource::Server);
    assert_eq!(expires_at, now + chrono::Duration::seconds(20));
}
#[test]
fn auth_expiry_overflow_is_checked_without_discarding_usable_server_values() {
    use auth::{ExpirySource, expiry};
    let now = chrono::DateTime::<chrono::Utc>::MAX_UTC - chrono::Duration::seconds(10);
    assert_eq!(
        expiry(&serde_json::json!({}), now),
        (now, ExpirySource::LocalPolicy)
    );
    let response = serde_json::json!({"cloudDragonTokens":{"expiresIn":1}});
    assert_eq!(
        expiry(&response, now),
        (now + chrono::Duration::seconds(1), ExpirySource::Server)
    );
}
fn empty() -> Lock {
    Lock {
        schema_version: SCHEMA,
        resolver_version: RESOLVER.into(),
        manifest_digest: digest(b"manifest"),
        roots: BTreeMap::new(),
        packages: BTreeMap::new(),
    }
}
#[test]
fn domain_digest_names_errors_and_schema() {
    assert_eq!(
        digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        json_digest(&BTreeMap::<String, String>::new()).unwrap(),
        digest(b"{}")
    );
    for name in ["", ".", "..", "a/b", "CON", "com1.txt", "trailing."] {
        assert!(!safe_name(name));
    }
    assert!(safe_name("code-review"));
    assert!(is_hex(&"a".repeat(40), 40));
    assert!(!is_hex("xx", 2));
    let e = Error::new("TEST", "message", 2)
        .phase("resolve")
        .package("package")
        .hint("retry");
    assert_eq!(e.to_string(), "TEST: message");
    assert_eq!(e.exit_code, 2);
    assert!(empty().validate().is_ok());
    let mut lock = empty();
    lock.schema_version = 9;
    assert_eq!(lock.validate().unwrap_err().code, "SCHEMA_VERSION");
}
#[test]
fn revision_identity_never_satisfies_semver() {
    let revision = Candidate {
        version: None,
        revision: Some("a".repeat(40)),
        selector: "main".into(),
    };
    assert!(!revision.matches(&Dependency {
        version: Some("*".into()),
        ..Dependency::default()
    }));
    assert!(revision.matches(&Dependency {
        rev: Some("main".into()),
        ..Dependency::default()
    }));
    let version = Candidate {
        version: Some("1.2.0-beta.1".into()),
        revision: None,
        selector: "latest".into(),
    };
    assert!(!version.matches(&Dependency {
        version: Some("^1".into()),
        ..Dependency::default()
    }));
    assert!(version.matches(&Dependency {
        version: Some("=1.2.0-beta.1".into()),
        ..Dependency::default()
    }));
}

#[test]
fn invalid_graph_references_and_case_conflicts_are_pure_failures() {
    let mut lock = empty();
    let source = Source::Archive {
        url: "https://example.org/skill.zip".into(),
    };
    let id = source.id();
    let package = LockedPackage {
        source,
        candidate: Candidate {
            version: Some("1.0.0".into()),
            revision: None,
            selector: "=1.0.0".into(),
        },
        metadata: Metadata {
            name: "example".into(),
            license: None,
            description: None,
            dependency_metadata: MetadataKind::Unknown,
            metadata_digest: None,
            dependencies: BTreeMap::new(),
            diagnostics: vec![],
        },
        tree_sha256: tree_digest(&fixture_files()).unwrap(),
        tree_algorithm: "skill-tree-sha256-v1".into(),
        files: fixture_files(),
        evidence: Evidence::default(),
        dependencies: BTreeMap::new(),
        directory: "example".into(),
        acquisition: Dependency::default(),
    };
    lock.packages.insert(id.clone(), package);
    lock.roots.insert(
        "root".into(),
        Edge {
            package: id.clone(),
            request: "=1.0.0".into(),
        },
    );
    lock.validate().unwrap();
    assert_eq!(
        lock.paths_to(&id).unwrap(),
        vec![vec!["root".to_string(), id.clone()]]
    );
    lock.packages.get_mut(&id).unwrap().dependencies.insert(
        "self".into(),
        Edge {
            package: id.clone(),
            request: "=1.0.0".into(),
        },
    );
    assert_eq!(lock.validate().unwrap_err().code, "DEPENDENCY_CYCLE");
    lock.packages.get_mut(&id).unwrap().dependencies.clear();
    let mut other = lock.packages[&id].clone();
    other.source = Source::Archive {
        url: "https://other.example/skill.zip".into(),
    };
    other.directory = "EXAMPLE".into();
    lock.packages.insert(other.source.id(), other);
    assert_eq!(lock.validate().unwrap_err().code, "DIRECTORY_CONFLICT");
}

fn fixture_files() -> Vec<FileRecord> {
    vec![FileRecord {
        path: "SKILL.md".into(),
        size: 0,
        sha256: digest(b""),
        executable: false,
    }]
}
