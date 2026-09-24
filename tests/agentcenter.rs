mod common;
use common::{
    archive,
    http::{Reply, Server},
};
use serde_json::json;
use skill_bom::{
    config::Manifest,
    domain::*,
    sources::{Provider, SourceProvider},
};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn manifest(url: &str, version: &str) -> Manifest {
    Manifest::parse(&format!(
        "schema_version=1\n[project]\nname='test'\n[registries.market]\nkind='agentcenter'\nurl={url:?}\ntoken_env='PATH'\n[dependencies.review]\nregistry='market'\npackage='review'\nversion={version:?}\n"
    )).unwrap()
}

fn server(latest: Arc<AtomicBool>, downloads: Arc<AtomicUsize>, bytes: Vec<u8>) -> Server {
    Server::new(move |path, request| {
        assert!(request.contains("X-Auth-Token:") || request.contains("x-auth-token:"));
        assert!(!request.contains("Authorization: Bearer"));
        if path.starts_with("/mcpService/external/skills/v1/get?") {
            assert!(request.starts_with("GET "));
            assert!(path.contains("skillId=review"));
            let version = if latest.load(Ordering::SeqCst) {
                "2.0.0"
            } else {
                "1.0.0"
            };
            return Reply::json(
                json!({"code":20000,"data":{"skillId":"review","latestVersion":version}}),
            );
        }
        assert_eq!(path, "/mcpService/external/skills/v1/download");
        assert!(request.starts_with("POST "));
        assert!(request.contains("\"skillId\":\"review\""));
        assert!(request.contains("\"version\":\"1.0.0\""));
        downloads.fetch_add(1, Ordering::SeqCst);
        Reply::bytes("application/zip", bytes.clone())
    })
}

#[test]
fn locks_installs_offline_and_recovers_historical_archive() {
    let latest = Arc::new(AtomicBool::new(false));
    let downloads = Arc::new(AtomicUsize::new(0));
    let bytes = archive("review", "1.0.0");
    let hash = digest(&bytes);
    let server = server(latest.clone(), downloads.clone(), bytes);
    let m = manifest(&server.url, "^1");
    let d = &m.dependencies["review"];
    let s = m.source(d).unwrap();
    assert_eq!(s.id(), format!("agentcenter:{}/skills/review#", server.url));
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mut provider = Provider::new(&m, cache.clone(), false, true).unwrap();
    let candidate = provider.candidates(&s, d).unwrap().remove(0);
    assert_eq!(candidate.version.as_deref(), Some("1.0.0"));
    let locked = provider.materialize(&s, &candidate, d, None).unwrap();
    assert_eq!(locked.metadata.name, "review");
    assert_eq!(
        locked.evidence.archive_sha256.as_deref(),
        Some(hash.as_str())
    );
    assert_eq!(downloads.load(Ordering::SeqCst), 1);
    let mut offline = Provider::new(&m, cache.clone(), true, true).unwrap();
    offline.ensure(&locked).unwrap();
    assert_eq!(downloads.load(Ordering::SeqCst), 1);
    let installed = provider.store.get(&locked).unwrap();
    std::fs::write(installed.join("SKILL.md"), "tampered").unwrap();
    assert!(offline.ensure(&locked).is_err());
    latest.store(true, Ordering::SeqCst);
    provider.ensure(&locked).unwrap();
    assert_eq!(downloads.load(Ordering::SeqCst), 2);
    assert_eq!(provider.candidates(&s, d).unwrap()[0], candidate); // process catalog cache
    let mut cold = Provider::new(&m, tmp.path().join("cold"), false, true).unwrap();
    assert_eq!(
        cold.materialize(&s, &candidate, d, None).unwrap_err().code,
        "SOURCE_VERSION_UNAVAILABLE"
    );
}

#[test]
fn config_identity_and_protocol_errors_fail_closed() {
    let base = "http://127.0.0.1:9999";
    assert!(Manifest::parse(&format!("schema_version=1\n[project]\nname='test'\n[registries.a]\nkind='agentcenter'\nurl='{base}'\n")).is_err());
    assert!(Manifest::parse(&format!("schema_version=1\n[project]\nname='test'\n[registries.a]\nkind='agentcenter'\nurl='{base}/path'\ntoken_env='PATH'\n")).is_err());
    for package in ["@owner/review", "../review", ""] {
        let text = format!(
            "schema_version=1\n[project]\nname='test'\n[registries.a]\nkind='agentcenter'\nurl='{base}'\ntoken_env='PATH'\n[dependencies.x]\nregistry='a'\npackage={package:?}\nversion='=1.0.0'\n"
        );
        assert!(Manifest::parse(&text).is_err());
    }
    let bad = Server::new(|_, _| Reply::json(json!({"code":40100,"data":{}})));
    let m = manifest(&bad.url, "=1.0.0");
    let d = &m.dependencies["review"];
    let s = m.source(d).unwrap();
    assert_eq!(
        skill_bom::agentcenter::candidates(&skill_bom::net::Http::new(false).unwrap(), &m, &s, d)
            .unwrap_err()
            .code,
        "AUTH_REQUIRED"
    );
    let missing = Manifest::parse(&format!("schema_version=1\n[project]\nname='test'\n[registries.a]\nkind='agentcenter'\nurl={:?}\ntoken_env='SKILL_BOM_UNSET_TEST_TOKEN_57'\n[dependencies.review]\nregistry='a'\npackage='review'\nversion='=1.0.0'\n", bad.url)).unwrap();
    let d = &missing.dependencies["review"];
    assert_eq!(
        skill_bom::agentcenter::candidates(
            &skill_bom::net::Http::new(false).unwrap(),
            &missing,
            &missing.source(d).unwrap(),
            d
        )
        .unwrap_err()
        .code,
        "MISSING_AUTH_TOKEN"
    );
}

#[test]
fn rejects_identity_nonzip_and_unsafe_zip() {
    for reply in [
        Reply::json(json!({"success":true,"data":{"skillId":"other","latestVersion":"1.0.0"}})),
        Reply::json(json!({"success":true,"data":{"skillId":"review","latestVersion":"n/a"}})),
    ] {
        let server = Server::new(move |_, _| Reply {
            status: reply.status,
            content_type: reply.content_type.clone(),
            bytes: reply.bytes.clone(),
            headers: vec![],
        });
        let m = manifest(&server.url, "=1.0.0");
        let d = &m.dependencies["review"];
        assert!(
            skill_bom::agentcenter::candidates(
                &skill_bom::net::Http::new(false).unwrap(),
                &m,
                &m.source(d).unwrap(),
                d
            )
            .is_err()
        );
    }
    let server = Server::new(|path, _| {
        if path.contains("/get?") {
            Reply::json(json!({"success":true,"data":{"skillId":"review","latestVersion":"1.0.0"}}))
        } else {
            Reply::bytes("text/plain", b"not zip".to_vec())
        }
    });
    let m = manifest(&server.url, "=1.0.0");
    let d = &m.dependencies["review"];
    let s = m.source(d).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut provider = Provider::new(&m, tmp.path().join("cache"), false, true).unwrap();
    let c = provider.candidates(&s, d).unwrap().remove(0);
    assert_eq!(
        provider.materialize(&s, &c, d, None).unwrap_err().code,
        "PROTOCOL"
    );
    let unsafe_zip = common::zip(&[("../escape", b"bad")]);
    let unsafe_server = Server::new(move |path, _| {
        if path.contains("/get?") {
            Reply::json(json!({"success":true,"data":{"skillId":"review","latestVersion":"1.0.0"}}))
        } else {
            Reply::bytes("application/zip", unsafe_zip.clone())
        }
    });
    let m = manifest(&unsafe_server.url, "=1.0.0");
    let d = &m.dependencies["review"];
    let s = m.source(d).unwrap();
    let mut provider = Provider::new(&m, tmp.path().join("cache2"), false, true).unwrap();
    let c = provider.candidates(&s, d).unwrap().remove(0);
    assert!(provider.materialize(&s, &c, d, None).is_err());
}

#[test]
fn cli_lock_install_verify_and_export_bom() {
    let latest = Arc::new(AtomicBool::new(false));
    let downloads = Arc::new(AtomicUsize::new(0));
    let server = server(latest, downloads.clone(), archive("review", "1.0.0"));
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("skills.toml"), format!(
        "schema_version=1\n[project]\nname='test'\n[registries.market]\nkind='agentcenter'\nurl={:?}\ntoken_env='PATH'\n[dependencies.review]\nregistry='market'\npackage='review'\nversion='=1.0.0'\n", server.url
    )).unwrap();
    let run = |args: &[&str]| {
        let binary = std::path::Path::new(env!("CARGO_BIN_EXE_skill-bom"));
        let binary = if binary.is_absolute() {
            binary.to_path_buf()
        } else {
            std::env::current_dir().unwrap().join(binary)
        };
        let output = Command::new(binary)
            .args(args)
            .current_dir(temp.path())
            .env("SKILL_BOM_HOME", temp.path().join("home"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    assert_eq!(run(&["validate", "--format", "json"])["valid"], true);
    assert_eq!(run(&["lock", "--format", "json"])["packages"], 1);
    let lock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(temp.path().join("skills.lock")).unwrap()).unwrap();
    let node = &lock["packages"][format!("agentcenter:{}/skills/review#", server.url)];
    assert_eq!(node["candidate"]["version"], "1.0.0");
    assert!(node["evidence"]["archive_sha256"].as_str().is_some());
    let requests = server.requests.load(Ordering::SeqCst);
    run(&["install", "--frozen", "--format", "json"]);
    assert_eq!(server.requests.load(Ordering::SeqCst), requests);
    assert!(temp.path().join("skills/review/SKILL.md").is_file());
    assert_eq!(run(&["verify", "--format", "json"])["lock_differs"], false);
    let bom = run(&["bom", "--format", "json"]);
    assert_eq!(bom["view"], "lock");
    assert_eq!(downloads.load(Ordering::SeqCst), 1);
}

#[test]
fn latest_mismatch_auth_failures_and_redirects_are_explicit() {
    let latest = Arc::new(AtomicBool::new(true));
    let server = server(
        latest,
        Arc::new(AtomicUsize::new(0)),
        archive("review", "1.0.0"),
    );
    let m = manifest(&server.url, "=1.0.0");
    let d = &m.dependencies["review"];
    let http = skill_bom::net::Http::new(false).unwrap();
    assert_eq!(
        skill_bom::agentcenter::candidates(&http, &m, &m.source(d).unwrap(), d)
            .unwrap_err()
            .code,
        "SOURCE_VERSION_UNAVAILABLE"
    );

    for status in [401, 403] {
        let rejected = Server::new(move |_, _| Reply::status(status, "private service error"));
        let m = manifest(&rejected.url, "=1.0.0");
        let d = &m.dependencies["review"];
        assert_eq!(
            skill_bom::agentcenter::candidates(&http, &m, &m.source(d).unwrap(), d)
                .unwrap_err()
                .code,
            "AUTH_REQUIRED"
        );
    }

    let target = Server::new(|_, _| Reply::json(json!({"success":true,"data":{}})));
    let redirect = Server::new({
        let location = format!("{}/elsewhere", target.url);
        move |_, _| Reply {
            status: 302,
            content_type: "text/plain".into(),
            bytes: vec![],
            headers: vec![("Location".into(), location.clone())],
        }
    });
    let m = manifest(&redirect.url, "=1.0.0");
    let d = &m.dependencies["review"];
    assert_eq!(
        skill_bom::agentcenter::candidates(&http, &m, &m.source(d).unwrap(), d)
            .unwrap_err()
            .code,
        "PROTOCOL"
    );
    assert_eq!(target.requests.load(Ordering::SeqCst), 0);
}

#[test]
fn explicit_subdir_and_business_envelope_are_checked() {
    let bytes = common::zip(&[
        ("wrapper/SKILL.md", b"---\nname: review\n---\n"),
        (
            "wrapper/skill.toml",
            common::metadata("review", "1.0.0", "").as_bytes(),
        ),
    ]);
    let server = Server::new(move |path, _| {
        if path.contains("/get?") {
            Reply::json(json!({"success":true,"data":{"skillId":"review","latestVersion":"1.0.0"}}))
        } else {
            Reply::bytes("application/octet-stream", bytes.clone())
        }
    });
    let mut m = manifest(&server.url, "=1.0.0");
    let d = m.dependencies.get_mut("review").unwrap();
    d.subdir = Some("wrapper".into());
    let d = &m.dependencies["review"];
    let s = m.source(d).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut p = Provider::new(&m, tmp.path().join("cache"), false, true).unwrap();
    let c = p.candidates(&s, d).unwrap().remove(0);
    assert_eq!(
        p.materialize(&s, &c, d, None).unwrap().metadata.name,
        "review"
    );

    for response in [
        json!({"code":50000,"data":{}}),
        json!({"success":false,"code":40300,"data":{}}),
        json!({"success":true,"data":[]}),
        json!({"success":true,"data":{"skillId":"review"}}),
    ] {
        let server = Server::new(move |_, _| Reply::json(response.clone()));
        let m = manifest(&server.url, "=1.0.0");
        let d = &m.dependencies["review"];
        assert!(
            skill_bom::agentcenter::candidates(
                &skill_bom::net::Http::new(false).unwrap(),
                &m,
                &m.source(d).unwrap(),
                d
            )
            .is_err()
        );
    }
}

#[test]
fn download_rate_limit_and_business_auth_errors_are_handled() {
    let tries = Arc::new(AtomicUsize::new(0));
    let zip = archive("review", "1.0.0");
    let server = Server::new({
        let tries = tries.clone();
        move |path, _| {
            if path.contains("/get?") {
                return Reply::json(
                    json!({"code":0,"data":{"skillId":"review","latestVersion":"1.0.0"}}),
                );
            }
            if tries.fetch_add(1, Ordering::SeqCst) == 0 {
                return Reply {
                    status: 429,
                    content_type: "text/plain".into(),
                    bytes: b"slow down".to_vec(),
                    headers: vec![("Retry-After".into(), "0".into())],
                };
            }
            Reply::bytes("application/zip", zip.clone())
        }
    });
    let m = manifest(&server.url, "=1.0.0");
    let d = &m.dependencies["review"];
    let s = m.source(d).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut p = Provider::new(&m, temp.path().join("cache"), false, true).unwrap();
    let c = p.candidates(&s, d).unwrap().remove(0);
    p.materialize(&s, &c, d, None).unwrap();
    assert_eq!(tries.load(Ordering::SeqCst), 2);

    let denied = Server::new(|path, _| {
        if path.contains("/get?") {
            Reply::json(json!({"success":true,"data":{"skillId":"review","latestVersion":"1.0.0"}}))
        } else {
            Reply::json(json!({"code":40300,"success":false,"data":{}}))
        }
    });
    let m = manifest(&denied.url, "=1.0.0");
    let d = &m.dependencies["review"];
    let s = m.source(d).unwrap();
    let mut p = Provider::new(&m, temp.path().join("cache2"), false, true).unwrap();
    let c = p.candidates(&s, d).unwrap().remove(0);
    assert_eq!(
        p.materialize(&s, &c, d, None).unwrap_err().code,
        "AUTH_REQUIRED"
    );
}
