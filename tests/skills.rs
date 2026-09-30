mod common;
use common::*;
use skill_bom::{
    config,
    domain::*,
    installer, sources,
    store::{self, Store},
};

fn parse(header: &str) -> Result<SkillFrontmatter> {
    config::skill::parse(format!("---\n{header}\n---\nBody stays opaque.\n").as_bytes())
}

#[test]
fn yaml_fields_extensions_and_original_resources() {
    let skill = parse("name: code-review\ndescription: >-\n  Review code:\n  when asked. # literal text\nlicense: MIT\ncompatibility: Requires git\nallowed-tools: Read Bash(git:*)\nmetadata:\n  version: '1.2'\ndisable-model-invocation: true\ncustom:\n  items: [one, two]").unwrap();
    assert_eq!(skill.description, "Review code: when asked. # literal text");
    assert_eq!(skill.metadata.unwrap()["version"], "1.2");
    assert_eq!(skill.extensions["custom"]["items"][1], "two");
    assert_eq!(skill.allowed_tools.as_deref(), Some("Read Bash(git:*)"));
    let s = config::skill::parse(b"---\r\nname: 'review' # comment\r\ndescription: |\r\n  First\r\n  Second\r\n---\r\n!`never run`\r\n").unwrap();
    assert_eq!(s.description, "First\nSecond\n");
    assert_eq!(
        parse("name: review\ndescription: ${HOME}")
            .unwrap()
            .description,
        "${HOME}"
    );
    assert_eq!(
        parse("name: review\ndescription: &desc Useful\ncustom: *desc")
            .unwrap()
            .extensions["custom"],
        "Useful"
    );
}

#[test]
fn malformed_fields_and_ambiguous_yaml_fail() {
    for header in [
        "name: review",
        "description: Useful",
        "name: 123\ndescription: Useful",
        "name: review\ndescription: true",
        "name: review\ndescription: null",
        "name: review\ndescription: ' '",
        "name: review\ndescription: [one]",
        "name: review\ndescription: Useful\nname: duplicate",
        "name: review\ndescription: Useful\nmetadata: {version: 1}",
        "name: review\ndescription: Useful\nmetadata: {client: {nested: value}}",
        "name: review\ndescription: Useful\nlicense: null",
        "name: review\ndescription: Useful\nallowed-tools: [Read]",
        "name: review\ndescription: Useful\ncompatibility: ''",
        "name: review\ndescription: !include /etc/passwd",
        "name: review\ndescription: !custom text",
        "[one, two]",
        "name: review\ndescription: 'unclosed",
        "name: review\ndescription: a: b",
        "name: review\ndescription: Useful\nmetadata: null",
    ] {
        assert!(parse(header).is_err(), "{header}");
    }
    for bytes in [
        b"Body".as_slice(),
        b"---\nname: review\n",
        b"---not a delimiter\n---\n",
        b"\xff",
        b"---",
    ] {
        assert!(config::skill::parse(bytes).is_err());
    }
}

#[test]
fn field_boundaries_and_parser_budgets() {
    for length in [1024, 1025] {
        assert_eq!(
            parse(&format!(
                "name: review\ndescription: {}",
                "文".repeat(length)
            ))
            .is_ok(),
            length == 1024
        );
    }
    for length in [500, 501] {
        assert_eq!(
            parse(&format!(
                "name: review\ndescription: Useful\ncompatibility: {}",
                "文".repeat(length)
            ))
            .is_ok(),
            length == 500
        );
    }
    assert_eq!(
        config::skill::parse(&vec![b'a'; 1024 * 1024 + 1])
            .unwrap_err()
            .code,
        "RESOURCE_LIMIT"
    );
    assert_eq!(
        parse(&format!(
            "name: review\ndescription: {}",
            "a".repeat(64 * 1024)
        ))
        .unwrap_err()
        .code,
        "RESOURCE_LIMIT"
    );
    assert!(
        parse(&format!(
            "name: review\ndescription: Useful\nextra: {}0{}",
            "[".repeat(40),
            "]".repeat(40)
        ))
        .is_err()
    );
    assert!(
        parse(&format!(
            "name: review\ndescription: &x Useful\nextra: [{}]",
            "*x,".repeat(100)
        ))
        .is_err()
    );
    assert!(
        parse(&format!(
            "name: review\ndescription: Useful\nextra: [{}]",
            "0,".repeat(6000)
        ))
        .is_err()
    );
}

#[test]
fn standard_identity_cannot_be_overridden_by_toml_or_supplements() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: review\ndescription: Standard description\nlicense: MIT\n---\n",
    )
    .unwrap();
    let mut m = manifest();
    let d = dep("review", "=1.0.0");
    let source = m.source(&d).unwrap();
    let candidate = Candidate {
        version: Some("1.0.0".into()),
        revision: Some("a".repeat(40)),
        selector: "v1.0.0".into(),
    };
    let meta = sources::metadata(&m, &source, &candidate, tmp.path(), false).unwrap();
    assert_eq!(meta.license.as_deref(), Some("MIT"));
    assert_eq!(meta.description.as_deref(), Some("Standard description"));
    assert!(!meta.complete());
    m.package_metadata.push(config::Supplement {
        source: d,
        name: "other".into(),
        complete: true,
        dependencies: Default::default(),
    });
    assert_eq!(
        sources::metadata(&m, &source, &candidate, tmp.path(), false)
            .unwrap_err()
            .code,
        "SKILL_NAME_MISMATCH"
    );
    m.package_metadata[0].name = "review".into();
    assert!(
        sources::metadata(&m, &source, &candidate, tmp.path(), true)
            .unwrap()
            .complete()
    );
    m.package_metadata.clear();
    std::fs::write(
        tmp.path().join("skill.toml"),
        metadata("other", "1.0.0", ""),
    )
    .unwrap();
    assert_eq!(
        sources::metadata(&m, &source, &candidate, tmp.path(), false)
            .unwrap_err()
            .code,
        "SKILL_NAME_MISMATCH"
    );
    std::fs::write(tmp.path().join("skill.toml"), "schema_version=1\n[package]\nname='review'\nversion='1.0.0'\ndescription='Old description'\n").unwrap();
    let meta = sources::metadata(&m, &source, &candidate, tmp.path(), true).unwrap();
    assert_eq!(meta.description.as_deref(), Some("Standard description"));
    assert_eq!(meta.license.as_deref(), Some("MIT"));
    std::fs::write(tmp.path().join("SKILL.md"), "---\nname: review\n---\n").unwrap();
    assert!(sources::metadata(&m, &source, &candidate, tmp.path(), false).is_err());
}

#[test]
fn old_cache_and_installed_records_are_auditable_but_not_redeployable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = Store::new(tmp.path().join("cache"));
    let mut old = package(&cache, "review", "1.0.0");
    let raw = tempfile::tempdir().unwrap();
    std::fs::write(
        raw.path().join("SKILL.md"),
        "---\nname: review\n---\nlegacy",
    )
    .unwrap();
    (old.tree_sha256, old.files) = cache.publish(raw.path()).unwrap();
    let m = manifest();
    for offline in [true, false] {
        let mut provider = sources::Provider::new(&m, cache.root.clone(), offline, false).unwrap();
        assert_eq!(provider.ensure(&old).unwrap_err().code, "SKILL_FORMAT");
    }
    let old_lock = lock(vec![old.clone()]);
    old_lock.validate().unwrap();
    let target = tmp.path().join("target");
    let guard = installer::acquire(&target, "owner").unwrap();
    store::copy_tree(
        &cache.get(&old).unwrap(),
        &target.join("review"),
        &old.files,
    )
    .unwrap();
    let state = installer::Installed {
        schema_version: SCHEMA,
        owner: "owner".into(),
        lock_digest: json_digest(&old_lock).unwrap(),
        lock: old_lock.clone(),
    };
    std::fs::write(
        target.join(".skill-bom/state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    assert!(installer::read(&target, "owner").unwrap().is_some());
    assert!(
        !installer::verify(&target, &state, &old_lock)
            .unwrap()
            .clean()
    );
    assert!(installer::deploy(&target, "owner", &old_lock, &cache, &guard).is_err());
    let next = lock(vec![package(&cache, "review", "1.1.0")]);
    assert!(
        installer::plan(&target, "owner", &next)
            .unwrap()
            .conflicts
            .is_empty()
    );
    installer::deploy(&target, "owner", &next, &cache, &guard).unwrap();
    let current = installer::read(&target, "owner").unwrap().unwrap();
    assert!(installer::verify(&target, &current, &next).unwrap().clean());
}

#[test]
fn exact_entrypoint_and_deployment_name_are_required() {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["skill.md", "skills.md"] {
        std::fs::write(
            tmp.path().join(name),
            "---\nname: review\ndescription: Useful\n---\n",
        )
        .unwrap();
        assert_eq!(
            store::skill(tmp.path()).unwrap_err().code,
            "SKILL_ENTRYPOINT"
        );
        std::fs::remove_file(tmp.path().join(name)).unwrap();
    }
    std::fs::create_dir(tmp.path().join("SKILL.md")).unwrap();
    assert_eq!(
        store::skill(tmp.path()).unwrap_err().code,
        "SKILL_ENTRYPOINT"
    );
    std::fs::remove_dir(tmp.path().join("SKILL.md")).unwrap();
    std::fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: review\ndescription: Useful\n---\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("skills.md"),
        "An ordinary supplementary document",
    )
    .unwrap();
    assert_eq!(store::skill(tmp.path()).unwrap().name, "review");
    let cache = Store::new(tmp.path().join("cache"));
    let mut p = package(&cache, "review", "1.0.0");
    p.directory = "alias".into();
    assert_eq!(
        store::validate_skill(&cache.get(&p).unwrap(), &p)
            .unwrap_err()
            .code,
        "SKILL_NAME_MISMATCH"
    );
    p.directory = "review".into();
    p.metadata.name = "alias".into();
    assert!(store::validate_skill(&cache.get(&p).unwrap(), &p).is_err());
}
