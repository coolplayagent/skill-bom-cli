mod common;
use common::{
    http::{Reply, Server},
    *,
};
use skill_bom::{
    config::Registry,
    domain::*,
    net::{Http, Response, Transport},
    process,
    sources::{self, SourceProvider},
    store::{self, Store},
};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn archive_provider_reproducibility_offline_and_cache_repair() {
    let archive = archive("example", "1.0.0");
    let hash = digest(&archive);
    let server = Server::new(move |_, _| Reply::bytes("application/zip", archive.clone()));
    let temp = tempfile::tempdir().unwrap();
    let m = manifest();
    let d = Dependency {
        url: Some(format!("{}/skill.zip", server.url)),
        version: Some("=1.0.0".into()),
        sha256: Some(hash),
        ..Dependency::default()
    };
    let source = m.source(&d).unwrap();
    let mut p = sources::Provider::new(&m, temp.path().join("cache"), false, true).unwrap();
    let c = p.candidates(&source, &d).unwrap().remove(0);
    assert_eq!(p.candidates(&source, &d).unwrap(), vec![c.clone()]);
    let locked = p.materialize(&source, &c, &d, None).unwrap();
    assert_eq!(locked.metadata.name, "example");
    assert!(p.ensure(&locked).is_ok());
    let requests = server.requests.load(Ordering::SeqCst);
    let mut offline = sources::Provider::new(&m, temp.path().join("cache"), true, true).unwrap();
    assert!(offline.ensure(&locked).is_ok());
    assert!(offline.materialize(&source, &c, &d, Some(&locked)).is_ok());
    assert_eq!(server.requests.load(Ordering::SeqCst), requests);
    assert!(offline.candidates(&source, &d).is_err());
    let path = p.store.get(&locked).unwrap();
    std::fs::write(path.join("SKILL.md"), "tampered").unwrap();
    assert!(offline.ensure(&locked).is_err());
    assert!(p.ensure(&locked).is_ok());
    assert!(server.requests.load(Ordering::SeqCst) > requests);
    let mut bad = d.clone();
    bad.sha256 = Some("0".repeat(64));
    assert_eq!(
        p.materialize(&source, &c, &bad, None).unwrap_err().code,
        "CHECKSUM_MISMATCH"
    );
    let mut changed = locked.clone();
    changed.evidence.archive_sha256 = Some("0".repeat(64));
    std::fs::remove_dir_all(path).unwrap();
    assert_eq!(p.ensure(&changed).unwrap_err().code, "CONTENT_CHANGED");
    assert!(Store::new(temp.path().join("cache")).path("../x").is_err());
}
#[test]
fn archives_reject_traversal_links_duplicates_case_and_size() {
    let temp = tempfile::tempdir().unwrap();
    for name in [
        "../escape",
        "/absolute",
        "a\\b",
        ".skill-bom/control",
        "A/../b",
    ] {
        let bytes = zip(&[(name, b"bad")]);
        let dest = tempfile::tempdir_in(temp.path()).unwrap();
        assert!(store::extract(&bytes, dest.path()).is_err(), "{name}");
    }
    let bytes = zip(&[("a", b"one"), ("A", b"two")]);
    assert!(store::extract(&bytes, temp.path()).is_err());
    let bytes = zip(&[("Dir/a", b"one"), ("dir/b", b"two")]);
    let dest = tempfile::tempdir().unwrap();
    assert!(store::extract(&bytes, dest.path()).is_err());
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_link_name("/etc/passwd").unwrap();
    header.set_cksum();
    builder.append_data(&mut header, "link", &b""[..]).unwrap();
    let bytes = builder.into_inner().unwrap();
    let dest = tempfile::tempdir().unwrap();
    assert_eq!(
        store::extract(&bytes, dest.path()).unwrap_err().code,
        "ARCHIVE_LINK"
    );
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("large").unwrap();
    header.set_size(MAX_BYTES + 1);
    header.set_mode(0o644);
    header.set_cksum();
    builder.get_mut().extend_from_slice(header.as_bytes());
    let bytes = builder.into_inner().unwrap();
    let dest = tempfile::tempdir().unwrap();
    assert_eq!(
        store::extract(&bytes, dest.path()).unwrap_err().code,
        "RESOURCE_LIMIT"
    );
    let bytes = tar(&[("same", b"1"), ("same", b"2")], false);
    let dest = tempfile::tempdir().unwrap();
    assert_eq!(
        store::extract(&bytes, dest.path()).unwrap_err().code,
        "ARCHIVE_DUPLICATE"
    );
    for gzip in [false, true] {
        let bytes = tar(
            &[("SKILL.md", b"original"), ("nested/file", b"payload")],
            gzip,
        );
        let dest = tempfile::tempdir().unwrap();
        store::extract(&bytes, dest.path()).unwrap();
        assert_eq!(store::inventory(dest.path()).unwrap().len(), 2);
    }
    assert!(store::extract(b"not an archive", temp.path()).is_err());
}
#[test]
fn git_tags_subdirectories_revisions_and_moving_tags() {
    let repo = tempfile::tempdir().unwrap();
    process::git(Some(repo.path()), &["init", "--quiet"]).unwrap();
    std::fs::create_dir(repo.path().join("skill")).unwrap();
    std::fs::write(repo.path().join("skill/SKILL.md"), "original").unwrap();
    process::git(Some(repo.path()), &["add", "."]).unwrap();
    process::git(
        Some(repo.path()),
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
    process::git(Some(repo.path()), &["tag", "v1.0.0"]).unwrap();
    process::git(Some(repo.path()), &["tag", "skill-v1.0.0"]).unwrap();
    let repository = repo.path().to_str().unwrap();
    let mut d = dep("repo", "^1");
    let candidates = sources::git::candidates(repository, &d).unwrap();
    assert_eq!(candidates.len(), 1);
    let selected = &candidates[0];
    let dest = tempfile::tempdir().unwrap();
    sources::git::fetch(repository, "skill", selected, dest.path()).unwrap();
    assert_eq!(
        std::fs::read(dest.path().join("SKILL.md")).unwrap(),
        b"original"
    );
    d.tag_pattern = Some("skill-v{version}".into());
    assert_eq!(sources::git::candidates(repository, &d).unwrap().len(), 1);
    d.version = None;
    d.tag_pattern = None;
    d.rev = Some("v1.0.0".into());
    assert_eq!(
        sources::git::candidates(repository, &d).unwrap()[0].revision,
        selected.revision
    );
    d.rev = selected.revision.clone();
    assert_eq!(
        sources::git::candidates(repository, &d).unwrap()[0].revision,
        selected.revision
    );
    d.rev = Some("missing".into());
    assert!(sources::git::candidates(repository, &d).is_err());
    std::fs::write(repo.path().join("skill/SKILL.md"), "new").unwrap();
    process::git(Some(repo.path()), &["add", "."]).unwrap();
    process::git(
        Some(repo.path()),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "new",
        ],
    )
    .unwrap();
    process::git(Some(repo.path()), &["tag", "--force", "v1.0.0"]).unwrap();
    let dest = tempfile::tempdir().unwrap();
    sources::git::fetch(repository, "skill", selected, dest.path()).unwrap();
    assert_eq!(
        std::fs::read(dest.path().join("SKILL.md")).unwrap(),
        b"original"
    );
    let dest = tempfile::tempdir().unwrap();
    assert!(sources::git::fetch(repository, "missing", selected, dest.path()).is_err());
    let bad = Candidate {
        version: None,
        revision: None,
        selector: "main".into(),
    };
    assert!(sources::git::fetch(repository, "", &bad, dest.path()).is_err());
}
fn hub_manifest(url: &str) -> skill_bom::config::Manifest {
    let mut m = manifest();
    m.registries.insert(
        "hub".into(),
        Registry {
            kind: "clawhub".into(),
            url: url.into(),
            token_env: None,
        },
    );
    m
}
fn hub_dep() -> Dependency {
    Dependency {
        registry: Some("hub".into()),
        package: Some("@owner/example".into()),
        version: Some("^1".into()),
        ..Dependency::default()
    }
}
fn info() -> serde_json::Value {
    serde_json::json!({"skill":{"slug":"example","tags":{"latest":"1.0.0"}},"owner":{"handle":"owner"},"latestVersion":{"version":"1.0.0"},"moderation":{"isMalwareBlocked":false,"isSuspicious":true}})
}
#[test]
fn clawhub_pages_owner_queries_zip_and_per_file_evidence() {
    let archive = archive("example", "1.0.0");
    let dir = tempfile::tempdir().unwrap();
    store::extract(&archive, dir.path()).unwrap();
    let files = store::inventory(dir.path()).unwrap();
    let server = Server::new(move |path, _| {
        assert!(path.contains("ownerHandle=owner"));
        if path.starts_with("/api/v1/download") {
            assert!(path.contains("version=1.0.0"));
            return Reply::bytes("application/zip", archive.clone());
        }
        if path.starts_with("/api/v1/skills/example/versions/1.0.0") {
            return Reply::json(
                serde_json::json!({"skill":{"slug":"example"},"version":{"version":"1.0.0","files":files}}),
            );
        }
        if path.starts_with("/api/v1/skills/example/versions?") {
            return if path.contains("cursor=") {
                Reply::json(
                    serde_json::json!({"items":[{"version":"1.0.0"},{"version":"1.1.0"}],"nextCursor":null}),
                )
            } else {
                Reply::json(serde_json::json!({"items":[{"version":"1.0.0"}],"nextCursor":"page2"}))
            };
        }
        Reply::json(info())
    });
    let m = hub_manifest(&server.url);
    let d = hub_dep();
    let source = m.source(&d).unwrap();
    let http = Http::new(false).unwrap();
    let cs = sources::clawhub::candidates(&http, &m, &source, &d).unwrap();
    assert_eq!(cs.len(), 2);
    let temp = tempfile::tempdir().unwrap();
    let (_, _, e) = sources::clawhub::fetch(&http, &m, &source, &cs[0], temp.path()).unwrap();
    assert!(e.upstream_hash.is_some());
    assert_eq!(e.scan.unwrap().status, "suspicious");
    let mut tag = d.clone();
    tag.version = None;
    tag.tag = Some("latest".into());
    assert_eq!(
        sources::clawhub::candidates(&http, &m, &source, &tag).unwrap()[0]
            .version
            .as_deref(),
        Some("1.0.0")
    );
}
#[test]
fn clawhub_rejects_identity_changes_blocked_sources_protocol_and_pagination_loops() {
    for mode in [
        "identity",
        "blocked",
        "loop",
        "page-protocol",
        "bad-json",
        "version",
        "descriptor",
        "unknown",
    ] {
        let mode = mode.to_string();
        let label = mode.clone();
        let server = Server::new(move |path, _| {
            if path.contains("/download") {
                return if mode == "descriptor" {
                    Reply::json(serde_json::json!({"sourceRef":"public-github"}))
                } else {
                    Reply::bytes("text/plain", b"unknown".to_vec())
                };
            }
            if path.contains("/versions/") {
                return Reply::json(
                    serde_json::json!({"skill":{"slug":"example"},"version":{"version":if mode=="version" {"2.0.0"}else{"1.0.0"}}}),
                );
            }
            if path.contains("/versions?") {
                return match mode.as_str() {
                    "loop" => Reply::json(serde_json::json!({"items":[],"nextCursor":"repeat"})),
                    "page-protocol" => Reply::json(serde_json::json!({})),
                    "bad-json" => Reply::bytes("application/json", b"no".to_vec()),
                    _ => Reply::json(
                        serde_json::json!({"items":[{"version":"1.0.0"}],"nextCursor":null}),
                    ),
                };
            }
            let mut data = info();
            if mode == "identity" {
                data["owner"]["handle"] = "different".into();
            }
            if mode == "blocked" {
                data["moderation"]["isMalwareBlocked"] = true.into();
            }
            Reply::json(data)
        });
        let m = hub_manifest(&server.url);
        let d = hub_dep();
        let source = m.source(&d).unwrap();
        let http = Http::new(false).unwrap();
        let result = sources::clawhub::candidates(&http, &m, &source, &d).and_then(|cs| {
            let dest = tempfile::tempdir().unwrap();
            sources::clawhub::fetch(&http, &m, &source, &cs[0], dest.path()).map(|_| ())
        });
        assert!(result.is_err(), "{label}");
    }
}
struct Mock {
    data: BTreeMap<String, Vec<u8>>,
    descriptor: serde_json::Value,
    info: serde_json::Value,
}
impl Transport for Mock {
    fn get(&self, url: &str, token: Option<&str>) -> Result<Response> {
        let parsed = url::Url::parse(url).unwrap();
        if parsed.host_str() == Some("api.github.com") {
            assert!(token.is_none());
            return Ok(Response {
                bytes: self.data[url].clone(),
                content_type: "application/zip".into(),
            });
        }
        let value = if parsed.path() == "/api/v1/download" {
            &self.descriptor
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
fn github_handoff_verifies_fixed_snapshot_and_distinct_hash_algorithm() {
    let commit = "a".repeat(40);
    let archive = zip(&[("repo-root/skill/SKILL.md", b"hello")]);
    let temp = tempfile::tempdir().unwrap();
    store::extract(&archive, temp.path()).unwrap();
    let files = store::inventory(&temp.path().join("repo-root/skill")).unwrap();
    let hash = sources::clawhub::github_hash(&files).unwrap();
    assert_ne!(hash, store::tree_digest(&files).unwrap());
    let url = format!("https://api.github.com/repos/org/repo/zipball/{commit}");
    let mut mock = Mock {
        data: BTreeMap::from([(url.clone(), archive)]),
        descriptor: serde_json::json!({"sourceRef":"public-github","repo":"org/repo","commit":commit,"path":"skill","contentHash":hash,"archiveUrl":url}),
        info: info(),
    };
    mock.info["skill"]["tags"] = serde_json::json!({});
    mock.info["latestVersion"] = serde_json::Value::Null;
    let m = hub_manifest("https://hub.example");
    let mut d = hub_dep();
    d.version = None;
    d.tag = Some("latest".into());
    let source = m.source(&d).unwrap();
    let c = sources::clawhub::candidates(&mock, &m, &source, &d)
        .unwrap()
        .remove(0);
    let dest = tempfile::tempdir().unwrap();
    let (_, selected, evidence) =
        sources::clawhub::fetch(&mock, &m, &source, &c, dest.path()).unwrap();
    assert_eq!(selected.revision.as_deref(), Some(commit.as_str()));
    assert_eq!(evidence.handoff.unwrap().path, "skill");
    mock.descriptor["contentHash"] = "0".repeat(64).into();
    let dest = tempfile::tempdir().unwrap();
    assert_eq!(
        sources::clawhub::fetch(&mock, &m, &source, &c, dest.path())
            .unwrap_err()
            .code,
        "CHECKSUM_MISMATCH"
    );
    mock.descriptor["archiveUrl"] = "https://evil.example/steal".into();
    let dest = tempfile::tempdir().unwrap();
    assert_eq!(
        sources::clawhub::fetch(&mock, &m, &source, &c, dest.path())
            .unwrap_err()
            .code,
        "PROTOCOL"
    );
}
#[test]
fn http_rate_limits_text_errors_redirect_credentials_and_offline() {
    let count = Arc::new(AtomicUsize::new(0));
    let shared = count.clone();
    let server = Server::new(move |_, _| {
        if shared.fetch_add(1, Ordering::SeqCst) == 0 {
            let mut r = Reply::status(429, "retry later");
            r.headers.push(("Retry-After".into(), "0".into()));
            r
        } else {
            Reply::json(serde_json::json!({"ok":true}))
        }
    });
    let http = Http::new(false).unwrap();
    assert_eq!(http.json(&server.url, None).unwrap()["ok"], true);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let target = Server::new(|_, headers| {
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        Reply::bytes("text/plain", b"done".to_vec())
    });
    let target_url = target.url.clone();
    let redirect = Server::new(move |_, headers| {
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer secret")
        );
        let mut r = Reply::status(302, "");
        r.headers.push(("Location".into(), target_url.clone()));
        r
    });
    assert_eq!(
        http.get(&redirect.url, Some("secret")).unwrap().bytes,
        b"done"
    );
    let offline = Http::new(true).unwrap();
    assert_eq!(
        offline.get(&server.url, None).err().unwrap().code,
        "OFFLINE_MISS"
    );
    for status in [403, 404, 423, 500, 429] {
        let server = Server::new(move |_, _| {
            let mut r = Reply::status(status, "plain text with secret");
            r.headers.push(("Retry-After".into(), "0".into()));
            r
        });
        let e = http.get(&server.url, None).err().unwrap();
        assert!(!e.message.contains("secret"));
    }
    let server = Server::new(|_, _| {
        let mut r = Reply::status(429, "retry");
        r.headers.push(("Retry-After".into(), "1000".into()));
        r
    });
    assert_eq!(
        http.get(&server.url, None).err().unwrap().code,
        "RATE_LIMIT"
    );
}

#[test]
#[ignore = "public ClawHub contract smoke; requires a reachable live service"]
fn live_clawhub_smoke() {
    let http = Http::new(false).unwrap();
    let catalog = http
        .json("https://clawhub.ai/api/v1/skills?limit=1", None)
        .unwrap();
    assert!(
        catalog
            .get("items")
            .and_then(serde_json::Value::as_array)
            .is_some(),
        "unexpected live catalog shape"
    );
}
