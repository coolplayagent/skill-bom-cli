mod common;
use common::*;
use skill_bom::{domain::*, resolver::Resolver, sources::SourceProvider};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default)]
struct Catalog {
    packages: BTreeMap<String, Vec<LockedPackage>>,
    queries: usize,
    offline: bool,
}
impl Catalog {
    fn add(&mut self, name: &str, version: &str, deps: &[(&str, &str)]) {
        let d = dep(name, &format!("={version}"));
        let source = manifest().source(&d).unwrap();
        let dependencies = deps
            .iter()
            .map(|(n, v)| (n.to_string(), dep(n, v)))
            .collect();
        let p = LockedPackage {
            source: source.clone(),
            candidate: Candidate {
                version: Some(version.into()),
                revision: Some("a".repeat(40)),
                selector: format!("v{version}"),
            },
            metadata: Metadata {
                name: name.into(),
                license: None,
                description: None,
                dependency_metadata: MetadataKind::Upstream,
                metadata_digest: None,
                dependencies,
                diagnostics: vec![],
            },
            tree_sha256: tree_digest(&fixture_files()).unwrap(),
            tree_algorithm: "skill-tree-sha256-v1".into(),
            files: fixture_files(),
            evidence: Evidence::default(),
            dependencies: BTreeMap::new(),
            directory: name.into(),
            acquisition: d,
        };
        self.packages.entry(source.id()).or_default().push(p);
    }
}
impl SourceProvider for Catalog {
    fn candidates(&mut self, s: &Source, _: &Dependency) -> Result<Vec<Candidate>> {
        self.queries += 1;
        if self.offline {
            return Err(Error::new("OFFLINE_MISS", "no network", 2));
        }
        Ok(self
            .packages
            .get(&s.id())
            .map(|p| p.iter().map(|p| p.candidate.clone()).collect())
            .unwrap_or_default())
    }
    fn materialize(
        &mut self,
        s: &Source,
        c: &Candidate,
        d: &Dependency,
        old: Option<&LockedPackage>,
    ) -> Result<LockedPackage> {
        let mut p = if let Some(p) = old.filter(|p| p.candidate == *c) {
            p.clone()
        } else {
            self.packages[&s.id()]
                .iter()
                .find(|p| p.candidate == *c)
                .unwrap()
                .clone()
        };
        p.acquisition = d.clone();
        Ok(p)
    }
}
#[test]
fn highest_intersection_prerelease_and_deterministic_backtracking() {
    let mut m = manifest();
    m.dependencies.insert("root".into(), dep("a", "^1"));
    m.dependencies.insert("shared".into(), dep("c", "=1.0.0"));
    let mut provider = Catalog::default();
    provider.add("a", "1.0.0", &[("b", "^1")]);
    provider.add("a", "1.1.0", &[("b", "^2")]);
    provider.add("a", "1.2.0-beta.1", &[]);
    provider.add("b", "1.0.0", &[("c", "^1")]);
    provider.add("b", "2.0.0", &[("c", "^2")]);
    provider.add("c", "1.0.0", &[]);
    provider.add("c", "2.0.0", &[]);
    let lock = Resolver::new(&m, &mut provider, None, BTreeSet::new())
        .resolve()
        .unwrap();
    let a = m.source(&m.dependencies["root"]).unwrap().id();
    assert_eq!(
        lock.packages[&a].candidate.version.as_deref(),
        Some("1.0.0")
    );
    assert_eq!(lock.packages.len(), 3);
    let c = m.source(&dep("c", "^1")).unwrap().id();
    assert_eq!(lock.paths_to(&c).unwrap().len(), 2);
    let again = Resolver::new(&m, &mut provider, None, BTreeSet::new())
        .resolve()
        .unwrap();
    assert_eq!(json_digest(&lock).unwrap(), json_digest(&again).unwrap());
    m.dependencies.remove("shared");
    m.dependencies
        .insert("root".into(), dep("a", "=1.2.0-beta.1"));
    let locked = Resolver::new(&m, &mut provider, None, BTreeSet::new())
        .resolve()
        .unwrap();
    assert_eq!(
        locked.packages[&a].candidate.version.as_deref(),
        Some("1.2.0-beta.1")
    );
}
#[test]
fn locks_are_preferred_and_updates_are_targeted() {
    let mut m = manifest();
    m.dependencies.insert("a".into(), dep("a", "^1"));
    m.dependencies.insert("b".into(), dep("b", "^1"));
    let mut p = Catalog::default();
    p.add("a", "1.0.0", &[]);
    p.add("b", "1.0.0", &[]);
    let first = Resolver::new(&m, &mut p, None, BTreeSet::new())
        .resolve()
        .unwrap();
    p.add("a", "1.1.0", &[]);
    p.add("b", "1.1.0", &[]);
    p.offline = true;
    let queries = p.queries;
    let kept = Resolver::new(&m, &mut p, Some(&first), BTreeSet::new())
        .resolve()
        .unwrap();
    assert_eq!(p.queries, queries);
    assert_eq!(json_digest(&first).unwrap(), json_digest(&kept).unwrap());
    p.offline = false;
    let a = m.source(&m.dependencies["a"]).unwrap().id();
    let b = m.source(&m.dependencies["b"]).unwrap().id();
    let updated = Resolver::new(&m, &mut p, Some(&first), BTreeSet::from([a.clone()]))
        .resolve()
        .unwrap();
    assert_eq!(
        updated.packages[&a].candidate.version.as_deref(),
        Some("1.1.0")
    );
    assert_eq!(
        updated.packages[&b].candidate.version.as_deref(),
        Some("1.0.0")
    );
}
#[test]
fn conflict_reports_every_root_chain_and_cycles_are_rejected() {
    let mut m = manifest();
    m.dependencies.insert("a".into(), dep("a", "^1"));
    m.dependencies.insert("b".into(), dep("b", "^1"));
    let mut p = Catalog::default();
    p.add("a", "1.0.0", &[("c", "^1")]);
    p.add("b", "1.0.0", &[("c", "^2")]);
    p.add("c", "1.0.0", &[]);
    p.add("c", "2.0.0", &[]);
    let error = Resolver::new(&m, &mut p, None, BTreeSet::new())
        .resolve()
        .unwrap_err();
    assert_eq!(error.code, "VERSION_CONFLICT");
    assert_eq!(error.chains.len(), 2);
    assert_eq!(error.chains[0][0], "a");
    assert_eq!(error.chains[1][0], "b");
    m.dependencies.remove("b");
    let mut p = Catalog::default();
    p.add("a", "1.0.0", &[("b", "^1")]);
    p.add("b", "1.0.0", &[("a", "^1")]);
    let error = Resolver::new(&m, &mut p, None, BTreeSet::new())
        .resolve()
        .unwrap_err();
    assert_eq!(error.code, "DEPENDENCY_CYCLE");
    assert!(error.chains[0].len() >= 4);
}
#[test]
fn deployment_identity_is_independent_of_alias_and_revision_is_not_semver() {
    let mut m = manifest();
    m.dependencies.insert("alias-a".into(), dep("a", "^1"));
    m.dependencies.insert("alias-b".into(), dep("b", "^1"));
    let mut p = Catalog::default();
    p.add("a", "1.0.0", &[]);
    p.add("b", "1.0.0", &[]);
    for packages in p.packages.values_mut() {
        packages[0].directory = "same".into();
    }
    let error = Resolver::new(&m, &mut p, None, BTreeSet::new())
        .resolve()
        .unwrap_err();
    assert_eq!(error.code, "DIRECTORY_CONFLICT");
    let c = Candidate {
        version: None,
        revision: Some("a".repeat(40)),
        selector: "main".into(),
    };
    assert!(!c.matches(&dep("a", "*")));
    let d = Dependency {
        rev: Some("main".into()),
        git: Some("https://example.org/a".into()),
        ..Dependency::default()
    };
    assert!(c.matches(&d));
    assert_eq!(c.display(), "a".repeat(40));
}
#[test]
fn graph_resource_limit_is_not_an_empty_resolution() {
    let mut m = manifest();
    m.dependencies.insert("root".into(), dep("p000", "^1"));
    let mut p = Catalog::default();
    for i in 0..=MAX_PACKAGES {
        let next = format!("p{:03}", i + 1);
        p.add(&format!("p{i:03}"), "1.0.0", &[(&next, "^1")]);
    }
    let error = Resolver::new(&m, &mut p, None, BTreeSet::new())
        .resolve()
        .unwrap_err();
    assert_eq!(error.code, "RESOURCE_LIMIT");
}

fn fixture_files() -> Vec<FileRecord> {
    vec![FileRecord {
        path: "SKILL.md".into(),
        size: 0,
        sha256: digest(b""),
        executable: false,
    }]
}
