mod common;
use common::*;
use skill_bom::{config::*, domain::*, paths, sources, store};
use std::collections::BTreeMap;

#[test]
fn strict_toml_and_source_combinations() {
    for text in [
        "schema_version=2\n[project]\nname='test'",
        "schema_version=1\n[project]\nname='x'\nnaem='typo'",
        "schema_version=1\n[project]\nname='../x'",
    ] {
        assert!(Manifest::parse(text).is_err());
    }
    let error = Manifest::parse("schema_version=1\n[project]\nname='x'\nnaem='typo'").unwrap_err();
    assert!(error.message.contains("naem"));
    assert!(error.message.contains("line"));
    let m = manifest();
    for d in [
        Dependency::default(),
        Dependency {
            git: Some("https://example.org/repo".into()),
            ..Dependency::default()
        },
        Dependency {
            url: Some("https://example.org/a.zip".into()),
            version: Some("^1".into()),
            ..Dependency::default()
        },
        Dependency {
            registry: Some("missing".into()),
            package: Some("@a/b".into()),
            version: Some("^1".into()),
            ..Dependency::default()
        },
    ] {
        assert!(m.source(&d).is_err());
    }
    for (field, value) in [
        ("url", "https://example.org/a"),
        ("sha256", "abc"),
        ("tag", "latest"),
        ("package", "@x/y"),
        ("subdir", "../bad"),
        ("version", "not-a-version"),
        ("rev", "main"),
    ] {
        let mut value_d = serde_json::to_value(dep("a", "^1")).unwrap();
        value_d[field] = value.into();
        let d: Dependency = serde_json::from_value(value_d).unwrap();
        assert!(m.source(&d).is_err(), "{field}");
    }
    let mut d = dep("a", "^1");
    d.tag_pattern = Some("release-{version}".into());
    assert!(m.source(&d).is_ok());
    for pattern in ["no-placeholder", "{version}{version}", "-{version}"] {
        d.tag_pattern = Some(pattern.into());
        assert!(m.source(&d).is_err());
    }
    d.version = None;
    d.rev = Some("-bad".into());
    d.tag_pattern = None;
    assert!(m.source(&d).is_err());
    d.rev = Some("main".into());
    assert!(m.source(&d).is_ok());
}
#[test]
fn normalized_identity_credentials_and_scope() {
    assert_eq!(
        git_url("git@example.org:org/repo.git").unwrap(),
        git_url("ssh://git@example.org/org/repo").unwrap()
    );
    assert_eq!(
        git_url("https://EXAMPLE.org/repo.git/").unwrap(),
        "https://example.org/repo"
    );
    for bad in [
        "https://user:secret@example.org/a",
        "https://example.org/a?token=secret",
        "https://example.org/a#hash",
        "file:///tmp/x",
        "http://example.org/a",
        "not a URL",
    ] {
        assert!(web_url(bad).is_err());
    }
    for bad in [
        "ssh://user:secret@example.org/a",
        "ssh://user@example.org/a",
        "ssh:///",
        "/tmp/repo",
        "git@example.org:",
    ] {
        assert!(git_url(bad).is_err());
    }
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("skills.toml");
    std::fs::write(
        &file,
        "schema_version=1\n[project]\nname='x'\n[install]\ntarget='./custom'",
    )
    .unwrap();
    let mut scope = Scope::new(Some(&file), false, None).unwrap();
    let m = scope.load(false).unwrap();
    assert_eq!(scope.target, tmp.path().join("custom"));
    let other = Scope::new(Some(&file), false, Some(&tmp.path().join("override"))).unwrap();
    assert_eq!(scope.owner, other.owner);
    assert!(Scope::new(Some(&file), true, None).is_err());
    assert_ne!(Scope::new(None, true, None).unwrap().owner, scope.owner);
    let mut changed = m.clone();
    changed.project.name = "new".into();
    assert_ne!(m.digest().unwrap(), changed.digest().unwrap());
    assert!(read_lock(&tmp.path().join("missing")).is_err());
}
#[test]
fn registry_contract_and_supplement_validation() {
    let text = "schema_version=1\n[project]\nname='x'\n[registries.hub]\nkind='clawhub'\nurl='https://example.org'\n[dependencies.a]\nregistry='hub'\npackage='@OWNER/SLUG'\ntag='latest'\n";
    let m = Manifest::parse(text).unwrap();
    assert_eq!(
        m.source(&m.dependencies["a"]).unwrap().id(),
        "clawhub:https://example.org/@owner/slug"
    );
    for (old, new) in [
        ("kind='clawhub'", "kind='npm'"),
        ("package='@OWNER/SLUG'", "package='slug'"),
        ("package='@OWNER/SLUG'", "package='@a/../b'"),
        ("tag='latest'", "tag='bad/tag'"),
        ("tag='latest'", "tag='latest'\nsubdir='x'"),
        ("kind='clawhub'", "kind='clawhub'\ntoken_env='BAD NAME'"),
    ] {
        assert!(Manifest::parse(&text.replace(old, new)).is_err());
    }
    let supplement = "\n[[package_metadata]]\nname='skill'\ncomplete=true\n[package_metadata.source]\ngit='https://example.org/skill'\nversion='=1.0.0'\n";
    let with = Manifest::parse(&format!("{text}{supplement}")).unwrap();
    assert_eq!(with.package_metadata.len(), 1);
    assert!(Manifest::parse(&format!("{text}{supplement}{supplement}")).is_err());
    assert!(Manifest::parse(&format!("{text}{}", supplement.replace("=1.0.0", "^1"))).is_err());
    let mut deps = BTreeMap::new();
    deps.insert("../bad".into(), dep("a", "^1"));
    assert!(m.validate_deps(&deps).is_err());
    let deps = (0..=MAX_PACKAGES)
        .map(|i| (format!("p{i}"), dep("a", "^1")))
        .collect();
    assert!(m.validate_deps(&deps).is_err());
}
#[test]
fn metadata_unknown_supplements_and_version_checks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let m = manifest();
    let d = dep("a", "^1");
    let source = m.source(&d).unwrap();
    let candidate = Candidate {
        version: Some("1.0.0".into()),
        revision: Some("a".repeat(40)),
        selector: "v1.0.0".into(),
    };
    assert!(sources::metadata(&m, &source, &candidate, root, false).is_err());
    std::fs::write(
        root.join("SKILL.md"),
        "---\nname: example\nversion: 9.0.0\ndescription: 'example'\n---\nRun nothing.",
    )
    .unwrap();
    let meta = sources::metadata(&m, &source, &candidate, root, false).unwrap();
    assert!(!meta.complete());
    assert!(meta.diagnostics.len() >= 2);
    assert!(sources::metadata(&m, &source, &candidate, root, true).is_err());
    let mut supplemented = m.clone();
    supplemented.package_metadata.push(Supplement {
        source: dep("a", "=1.0.0"),
        name: "supplemented".into(),
        complete: true,
        dependencies: BTreeMap::new(),
    });
    assert!(
        sources::metadata(&supplemented, &source, &candidate, root, true)
            .unwrap()
            .complete()
    );
    supplemented.package_metadata[0].complete = false;
    assert!(sources::metadata(&supplemented, &source, &candidate, root, true).is_err());
    let bytes = metadata("example", "1.0.0", "");
    std::fs::write(root.join("skill.toml"), &bytes).unwrap();
    let meta = sources::metadata(&m, &source, &candidate, root, true).unwrap();
    assert_eq!(meta.metadata_digest, Some(digest(bytes.as_bytes())));
    assert!(meta.complete());
    assert!(sources::metadata(&supplemented, &source, &candidate, root, false).is_err());
    for bad in [
        metadata("example", "2.0.0", ""),
        metadata("../bad", "1.0.0", ""),
        metadata("example", "bad", ""),
        "schema_version=1\nunknown=true".into(),
        metadata("example", "1.0.0", "").replace("schema_version=1", "schema_version=99"),
    ] {
        std::fs::write(root.join("skill.toml"), bad).unwrap();
        assert!(sources::metadata(&m, &source, &candidate, root, false).is_err());
    }
    assert_eq!(
        store::entrypoint(root).unwrap().file_name().unwrap(),
        "SKILL.md"
    );
    std::fs::write(root.join("skill.md"), "extra").unwrap();
    let entry_names: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    if entry_names.iter().any(|name| name == "SKILL.md")
        && entry_names.iter().any(|name| name == "skill.md")
    {
        assert!(store::entrypoint(root).is_err());
    } else {
        assert!(store::entrypoint(root).is_ok());
    }
}
#[test]
fn portable_paths_and_atomic_writes() {
    for bad in [
        "",
        "/absolute",
        "a/../b",
        "a//b",
        "a\\b",
        "C:x",
        "a/.skill-bom/x",
        ".git/config",
        "a/CON.txt",
        "x.",
        "a\0b",
        "a/<b",
        "a/./b",
    ] {
        assert!(paths::relative(bad).is_err(), "{bad:?}");
    }
    assert!(paths::relative("nested/file.md").is_ok());
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("dir/file");
    paths::atomic_write(&path, b"first").unwrap();
    paths::atomic_write(&path, b"second").unwrap();
    assert_eq!(paths::read(&path, 6).unwrap(), b"second");
    assert!(paths::read(&path, 2).is_err());
    assert!(paths::read(t.path(), 100).is_err());
    assert_eq!(
        paths::absolute(&t.path().join("dir/../x")).unwrap(),
        t.path().join("x")
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&path, t.path().join("link")).unwrap();
        assert!(paths::read(&t.path().join("link"), 100).is_err());
        assert!(paths::atomic_write(&t.path().join("link"), b"no").is_err());
    }
}
