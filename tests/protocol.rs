mod common;
use common::{
    http::{Reply, Server},
    *,
};
use serde_json::{Value, json};
use skill_bom::{
    config::Registry,
    domain::*,
    installer,
    net::{Http, Response, Transport},
    sources::{self, SourceProvider},
    store::{self, Store},
};
use std::collections::BTreeMap;

struct Fixture {
    info: Value,
    page: Value,
    detail: Value,
    download: Value,
    archive: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            info: json!({"skill":{"slug":"example","tags":{"latest":"1.0.0"}},"owner":{"handle":"owner"},"latestVersion":{"version":"1.0.0"}}),
            page: json!({"items":[{"version":"junk"},{"version":"1.0.0"}],"nextCursor":null}),
            detail: json!({"skill":{"slug":"example"},"version":{"version":"1.0.0"}}),
            download: Value::Null,
            archive: archive("example", "1.0.0"),
        }
    }
    fn setup() -> (skill_bom::config::Manifest, Dependency, Source) {
        let mut m = manifest();
        m.registries.insert(
            "hub".into(),
            Registry {
                kind: "clawhub".into(),
                url: "https://hub.example".into(),
                token_env: Some("PATH".into()),
            },
        );
        let d = Dependency {
            registry: Some("hub".into()),
            package: Some("@owner/example".into()),
            version: Some("^1".into()),
            ..Dependency::default()
        };
        let source = m.source(&d).unwrap();
        (m, d, source)
    }
    fn candidate() -> Candidate {
        Candidate {
            version: Some("1.0.0".into()),
            revision: None,
            selector: "1.0.0".into(),
        }
    }
    fn fetch(&self) -> Result<(std::path::PathBuf, Candidate, Evidence)> {
        let (m, _, s) = Self::setup();
        let temp = tempfile::tempdir().unwrap();
        sources::clawhub::fetch(self, &m, &s, &Self::candidate(), temp.path())
    }
}
impl Transport for Fixture {
    fn get(&self, url: &str, token: Option<&str>) -> Result<Response> {
        let url = url::Url::parse(url).unwrap();
        if url.host_str() == Some("api.github.com") {
            assert!(token.is_none());
            return Ok(Response {
                bytes: self.archive.clone(),
                content_type: "application/zip".into(),
            });
        }
        assert!(token.is_some());
        let path = url.path();
        let value = if path == "/api/v1/download" {
            if self.download.is_null() {
                return Ok(Response {
                    bytes: self.archive.clone(),
                    content_type: "application/zip".into(),
                });
            }
            &self.download
        } else if path.ends_with("/versions") {
            &self.page
        } else if path.contains("/versions/") {
            &self.detail
        } else {
            &self.info
        };
        Ok(Response {
            bytes: serde_json::to_vec(value).unwrap(),
            content_type: "application/json".into(),
        })
    }
}
#[test]
fn clawhub_errors_preserve_identity_and_protocol_evidence() {
    let (m, d, s) = Fixture::setup();
    let mut f = Fixture::new();
    assert_eq!(
        sources::clawhub::candidates(&f, &m, &s, &d).unwrap().len(),
        1
    );
    f.page = json!({"items":[{}],"nextCursor":null});
    assert!(sources::clawhub::candidates(&f, &m, &s, &d).is_err());
    f.page = json!({"items":[],"nextCursor":false});
    assert!(sources::clawhub::candidates(&f, &m, &s, &d).is_err());
    f.page = json!({"items":(0..=MAX_CANDIDATES).map(|i|json!({"version":format!("1.0.{i}")})).collect::<Vec<_>>(),"nextCursor":null});
    assert_eq!(
        sources::clawhub::candidates(&f, &m, &s, &d)
            .unwrap_err()
            .code,
        "RESOURCE_LIMIT"
    );
    let mut tag = d.clone();
    tag.version = None;
    tag.tag = Some("missing".into());
    assert_eq!(
        sources::clawhub::candidates(&f, &m, &s, &tag)
            .unwrap_err()
            .code,
        "SOURCE_VERSION_UNAVAILABLE"
    );
    tag.tag = Some("latest".into());
    f.info["skill"]["tags"]["latest"] = "not-semver".into();
    assert!(sources::clawhub::candidates(&f, &m, &s, &tag).is_err());
    for status in ["malicious", "failed", "error", "pending"] {
        let mut f = Fixture::new();
        f.detail["version"]["security"] = json!({"status":status});
        assert_eq!(f.fetch().unwrap_err().code, "SOURCE_BLOCKED");
    }
    let mut f = Fixture::new();
    f.download = json!({"sourceRef":"unknown"});
    assert_eq!(f.fetch().unwrap_err().code, "PROTOCOL");
    let mut f = Fixture::new();
    f.download = json!({"sourceRef":"public-github"});
    assert_eq!(f.fetch().unwrap_err().code, "SOURCE_VERSION_UNAVAILABLE");
    for files in [
        json!("wrong type"),
        json!([{"path":"missing","sha256":"0".repeat(64)}]),
        json!([{"path":"SKILL.md","sha256":"0".repeat(64)}]),
        json!([]),
    ] {
        let mut f = Fixture::new();
        f.detail["version"]["files"] = files;
        assert!(f.fetch().is_err());
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut f = Fixture::new();
    store::extract(&f.archive, tmp.path()).unwrap();
    let files = store::inventory(tmp.path()).unwrap();
    let mut duplicates = serde_json::to_value(&files).unwrap();
    duplicates
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(&files[0]).unwrap());
    f.detail["version"]["files"] = duplicates;
    assert_eq!(f.fetch().unwrap_err().code, "PROTOCOL");
    let source = Source::Archive {
        url: "https://example.org/a".into(),
    };
    assert!(sources::clawhub::candidates(&f, &m, &source, &d).is_err());
}
#[test]
fn handoff_rejects_incomplete_moving_unsafe_and_wrong_root_descriptors() {
    let (m, _, s) = Fixture::setup();
    let c = Candidate {
        version: None,
        revision: None,
        selector: "latest".into(),
    };
    let commit = "a".repeat(40);
    let descriptor = json!({"sourceRef":"public-github","repo":"org/repo","commit":commit,"path":"skill","contentHash":"b".repeat(64),"archiveUrl":format!("https://api.github.com/repos/org/repo/zipball/{commit}")});
    for (key, bad) in [
        ("repo", json!("bad")),
        ("commit", json!("short")),
        ("path", json!("../bad")),
        ("contentHash", json!("invalid")),
        ("archiveUrl", json!("https://example.org/evil")),
    ] {
        let mut f = Fixture::new();
        f.download = descriptor.clone();
        f.download[key] = bad;
        let temp = tempfile::tempdir().unwrap();
        assert!(
            sources::clawhub::fetch(&f, &m, &s, &c, temp.path()).is_err(),
            "{key}"
        );
    }
    let mut f = Fixture::new();
    f.download = descriptor.clone();
    let mut moved = c.clone();
    moved.revision = Some("c".repeat(40));
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(
        sources::clawhub::fetch(&f, &m, &s, &moved, temp.path())
            .unwrap_err()
            .code,
        "SOURCE_VERSION_UNAVAILABLE"
    );
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(
        sources::clawhub::fetch(&f, &m, &s, &c, temp.path())
            .unwrap_err()
            .code,
        "PROTOCOL"
    );
    f.download = Value::Null;
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(
        sources::clawhub::fetch(&f, &m, &s, &c, temp.path())
            .unwrap_err()
            .code,
        "SOURCE_VERSION_UNAVAILABLE"
    );
}
#[test]
fn http_rejects_redirect_loops_bad_destinations_and_malformed_responses() {
    let http = Http::new(false).unwrap();
    assert!(http.get("not a url", None).is_err());
    for destination in [
        None,
        Some("/loop"),
        Some("http://remote.example/a"),
        Some("https://secret:password@example.org/a"),
        Some("http://[invalid"),
    ] {
        let server = Server::new(move |_, _| {
            let mut r = Reply::status(302, "");
            if let Some(d) = destination {
                r.headers.push(("Location".into(), d.into()));
            }
            r
        });
        assert!(http.get(&server.url, None).is_err());
    }
    let server = Server::new(|_, _| Reply::bytes("application/json", b"malformed".to_vec()));
    assert!(http.json(&server.url, None).is_err());
    let server = Server::new(|_, _| {
        Reply::bytes(
            "application/octet-stream",
            vec![0; (MAX_DOWNLOAD + 1) as usize],
        )
    });
    assert_eq!(
        http.get(&server.url, None).err().unwrap().code,
        "RESOURCE_LIMIT"
    );
}
#[test]
fn lock_schema_graph_and_checksum_invariants_reject_tampering() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Store::new(temp.path().join("cache"));
    let p = package(&cache, "example", "1.0.0");
    let good = lock(vec![p.clone()]);
    for mutation in 0..8 {
        let mut bad = good.clone();
        let id = p.source.id();
        match mutation {
            0 => bad.resolver_version = "unknown".into(),
            1 => bad.packages.get_mut(&id).unwrap().directory = "../escape".into(),
            2 => bad.packages.get_mut(&id).unwrap().tree_algorithm = "unknown".into(),
            3 => bad.packages.get_mut(&id).unwrap().candidate.revision = Some("short".into()),
            4 => {
                bad.packages.get_mut(&id).unwrap().candidate.revision = None;
                bad.packages.get_mut(&id).unwrap().candidate.version = None;
            }
            5 => {
                bad.packages.get_mut(&id).unwrap().dependencies.insert(
                    "missing".into(),
                    Edge {
                        package: "missing".into(),
                        request: "^1".into(),
                    },
                );
            }
            6 => bad.roots.clear(),
            _ => {
                bad.packages.get_mut(&id).unwrap().dependencies.insert(
                    "cycle".into(),
                    Edge {
                        package: id,
                        request: "^1".into(),
                    },
                );
            }
        }
        assert!(bad.validate().is_err(), "{mutation}");
    }
    let mut dangling = good.clone();
    dangling.roots.values_mut().next().unwrap().package = "missing".into();
    assert!(dangling.validate().is_err());
    assert!(good.paths_to("missing").unwrap().is_empty());
}
#[test]
fn transaction_recovery_validates_journal_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Store::new(temp.path().join("cache"));
    let target = temp.path().join("target");
    let good = lock(vec![package(&cache, "example", "1.0.0")]);
    let guard = installer::acquire(&target, "owner").unwrap();
    installer::deploy(&target, "owner", &good, &cache, &guard).unwrap();
    drop(guard);
    let state = installer::read(&target, "owner").unwrap().unwrap();
    let path = target.join(".skill-bom/transaction.json");
    let journal =
        json!({"schema_version":1,"committed":false,"old":state,"new":state,"changes":[]});
    for mutation in 0..4 {
        let mut j = journal.clone();
        match mutation {
            0 => j["new"]["owner"] = "other".into(),
            1 => j["old"]["lock_digest"] = "bad".into(),
            2 => j["changes"] = json!([{"directory":"../bad","action":"remove","package":"x"}]),
            _ => j["changes"] = json!([{"directory":"missing","action":"replace","package":"x"}]),
        };
        std::fs::write(&path, serde_json::to_vec(&j).unwrap()).unwrap();
        assert!(installer::acquire(&target, "owner").is_err());
        assert!(target.join("example/SKILL.md").is_file());
    }
    let mut j = journal;
    j["committed"] = true.into();
    std::fs::write(&path, serde_json::to_vec(&j).unwrap()).unwrap();
    let _guard = installer::acquire(&target, "owner").unwrap();
    assert!(!path.exists());
}
#[test]
fn lfs_symlink_and_subdirectory_acquisition_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("SKILL.md"),
        "version https://git-lfs.github.com/spec/v1\noid sha256:abc",
    )
    .unwrap();
    assert_eq!(
        store::inventory(temp.path()).unwrap_err().code,
        "LFS_UNMATERIALIZED"
    );
    #[cfg(unix)]
    {
        std::fs::remove_file(temp.path().join("SKILL.md")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", temp.path().join("SKILL.md")).unwrap();
        assert_eq!(
            store::inventory(temp.path()).unwrap_err().code,
            "UNSAFE_PATH"
        );
    }
    let bytes = tar(&[("root/SKILL.md", b"---\nname: sub\n---\nhello")], true);
    let hash = digest(&bytes);
    let server = Server::new(move |_, _| Reply::bytes("application/gzip", bytes.clone()));
    let m = manifest();
    let d = Dependency {
        url: Some(format!("{}/a.tar.gz", server.url)),
        version: Some("=1.0.0".into()),
        sha256: Some(hash),
        subdir: Some("root".into()),
        ..Dependency::default()
    };
    let source = m.source(&d).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut provider =
        sources::Provider::new(&m, cache.path().join("cache"), false, false).unwrap();
    let c = provider.candidates(&source, &d).unwrap().remove(0);
    assert_eq!(
        provider
            .materialize(&source, &c, &d, None)
            .unwrap()
            .directory,
        "sub"
    );
    let mut offline =
        sources::Provider::new(&m, cache.path().join("missing"), true, false).unwrap();
    assert_eq!(
        offline.materialize(&source, &c, &d, None).unwrap_err().code,
        "OFFLINE_MISS"
    );
    let no_front = tempfile::tempdir().unwrap();
    std::fs::write(no_front.path().join("skills.md"), "no frontmatter").unwrap();
    assert!(sources::metadata(&m, &source, &c, no_front.path(), false).is_err());
    std::fs::write(no_front.path().join("skills.md"), "---\nname: ../bad\n---").unwrap();
    assert!(sources::metadata(&m, &source, &c, no_front.path(), false).is_err());
    let mut m = m.clone();
    let supplement = skill_bom::config::Supplement {
        source: d.clone(),
        name: "sub".into(),
        complete: true,
        dependencies: BTreeMap::new(),
    };
    m.package_metadata = vec![supplement.clone(), supplement];
    assert!(sources::metadata(&m, &source, &c, no_front.path(), false).is_err());
}

#[test]
fn git_provider_locks_exact_content_and_rejects_gitlinks() {
    let repo = tempfile::tempdir().unwrap();
    let path = repo.path();
    skill_bom::process::git(Some(path), &["init", "--quiet"]).unwrap();
    std::fs::write(path.join("SKILL.md"), "---\nname: example\n---\nhello").unwrap();
    std::fs::write(path.join("skill.toml"), metadata("example", "1.0.0", "")).unwrap();
    skill_bom::process::git(Some(path), &["add", "."]).unwrap();
    skill_bom::process::git(
        Some(path),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    )
    .unwrap();
    skill_bom::process::git(
        Some(path),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "tag",
            "-a",
            "v1.0.0",
            "-m",
            "annotated",
        ],
    )
    .unwrap();
    let repository = path.to_str().unwrap();
    let request = dep("example", "^1");
    let source = Source::Git {
        repository: repository.into(),
        subdir: String::new(),
    };
    let m = manifest();
    let cache = tempfile::tempdir().unwrap();
    let mut provider = sources::Provider::new(&m, cache.path().join("cache"), false, true).unwrap();
    let c = provider.candidates(&source, &request).unwrap().remove(0);
    let p = provider.materialize(&source, &c, &request, None).unwrap();
    assert_eq!(p.metadata.name, "example");
    assert!(p.evidence.archive_sha256.is_none());
    let mut revision = request.clone();
    revision.version = None;
    revision.rev = Some("v1.0.0".into());
    assert_eq!(
        sources::git::candidates(repository, &revision).unwrap()[0].revision,
        c.revision
    );
    skill_bom::process::git(
        Some(path),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000",
            c.revision.as_deref().unwrap(),
            "unmaterialized",
        ],
    )
    .unwrap();
    skill_bom::process::git(
        Some(path),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "gitlink",
        ],
    )
    .unwrap();
    let commit =
        String::from_utf8(skill_bom::process::git(Some(path), &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_string();
    let c = Candidate {
        version: None,
        revision: Some(commit),
        selector: "HEAD".into(),
    };
    let dest = tempfile::tempdir().unwrap();
    assert_eq!(
        sources::git::fetch(repository, "", &c, dest.path())
            .unwrap_err()
            .code,
        "SUBMODULE_UNMATERIALIZED"
    );
}
