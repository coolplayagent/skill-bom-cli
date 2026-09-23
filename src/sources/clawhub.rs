//! ClawHub contract baseline: 826992bd72b9f9ab09254dc43551facdf94cb07b.
use crate::domain::*;
use crate::{config::Manifest, env, net::Transport, paths, store};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn parts(source: &Source) -> Result<(&str, &str, &str)> {
    if let Source::Clawhub {
        registry,
        owner,
        slug,
    } = source
    {
        Ok((registry, owner, slug))
    } else {
        fail("SOURCE", "Expected ClawHub identity")
    }
}
fn token(manifest: &Manifest, registry: &str) -> Option<String> {
    manifest
        .registries
        .values()
        .find(|r| r.url.trim_end_matches('/') == registry)
        .and_then(|r| r.token_env.as_ref())
        .and_then(|key| env::variable(key))
}
fn endpoint(source: &Source, suffix: &str, params: &[(&str, &str)]) -> Result<String> {
    let (registry, owner, slug) = parts(source)?;
    let path = if suffix == "download" {
        "/api/v1/download".into()
    } else {
        format!("/api/v1/skills/{slug}{suffix}")
    };
    let mut url = url::Url::parse(&format!("{registry}{path}"))
        .map_err(|_| Error::new("URL", "Invalid registry", 2))?;
    url.query_pairs_mut().append_pair("ownerHandle", owner);
    if suffix == "download" {
        url.query_pairs_mut().append_pair("slug", slug);
    }
    for (k, v) in params {
        url.query_pairs_mut().append_pair(k, v);
    }
    Ok(url.into())
}
fn info(http: &impl Transport, manifest: &Manifest, source: &Source) -> Result<Value> {
    let (registry, owner, slug) = parts(source)?;
    let t = token(manifest, registry);
    let value = http.json(&endpoint(source, "", &[])?, t.as_deref())?;
    if value.pointer("/owner/handle").and_then(Value::as_str) != Some(owner)
        || value.pointer("/skill/slug").and_then(Value::as_str) != Some(slug)
    {
        return fail(
            "SOURCE_IDENTITY_CHANGED",
            "ClawHub owner or canonical slug differs; update the declaration explicitly",
        );
    }
    if value
        .pointer("/moderation/isMalwareBlocked")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return fail("SOURCE_BLOCKED", "ClawHub blocked this skill");
    }
    Ok(value)
}
pub fn candidates(
    http: &impl Transport,
    manifest: &Manifest,
    source: &Source,
    request: &Dependency,
) -> Result<Vec<Candidate>> {
    let data = info(http, manifest, source)?;
    if let Some(tag) = &request.tag {
        let version = data
            .get("skill")
            .and_then(|s| s.get("tags"))
            .and_then(|tags| tags.get(tag))
            .and_then(Value::as_str)
            .or_else(|| {
                if tag == "latest" {
                    data.pointer("/latestVersion/version")
                        .and_then(Value::as_str)
                } else {
                    None
                }
            });
        if let Some(v) = version {
            semver::Version::parse(v).map_err(|_| {
                Error::new(
                    "SOURCE_VERSION_UNAVAILABLE",
                    "Tag does not identify a SemVer release",
                    1,
                )
            })?;
            return Ok(vec![Candidate {
                version: Some(v.into()),
                revision: None,
                selector: tag.clone(),
            }]);
        }
        if tag != "latest" {
            return fail("SOURCE_VERSION_UNAVAILABLE", "Unknown ClawHub tag");
        }
        return Ok(vec![Candidate {
            version: None,
            revision: None,
            selector: tag.clone(),
        }]);
    }
    let (registry, _, _) = parts(source)?;
    let t = token(manifest, registry);
    let mut cursor = String::new();
    let mut cursors = BTreeSet::new();
    let mut versions = BTreeSet::new();
    let mut result = vec![];
    for _ in 0..64 {
        let mut params = vec![("limit", "100")];
        if !cursor.is_empty() {
            params.push(("cursor", &cursor));
        }
        let page = http.json(&endpoint(source, "/versions", &params)?, t.as_deref())?;
        let items = page
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new("PROTOCOL", "Missing version page items", 2))?;
        for item in items {
            let version = required(item, "version")?;
            if semver::Version::parse(version).is_err() {
                continue;
            }
            if versions.insert(version.to_string()) {
                result.push(Candidate {
                    version: Some(version.into()),
                    revision: None,
                    selector: version.into(),
                });
            }
            if result.len() > MAX_CANDIDATES {
                return Err(Error::new(
                    "RESOURCE_LIMIT",
                    "Version catalog exceeds budget",
                    2,
                ));
            }
        }
        match page.get("nextCursor") {
            Some(Value::Null) => return Ok(result),
            Some(Value::String(next)) if !next.is_empty() => {
                if !cursors.insert(next.clone()) {
                    return Err(Error::new("PAGINATION_LOOP", "Repeated ClawHub cursor", 2));
                }
                cursor = next.clone();
            }
            _ => return Err(Error::new("PROTOCOL", "Missing or invalid nextCursor", 2)),
        }
    }
    Err(Error::new(
        "RESOURCE_LIMIT",
        "Version pagination exceeds 64 pages",
        2,
    ))
}
pub fn fetch(
    http: &impl Transport,
    manifest: &Manifest,
    source: &Source,
    candidate: &Candidate,
    dest: &Path,
) -> Result<(PathBuf, Candidate, Evidence)> {
    let data = info(http, manifest, source)?;
    let (registry, _, slug) = parts(source)?;
    let t = token(manifest, registry);
    let detail = if let Some(version) = &candidate.version {
        let value = http.json(
            &endpoint(source, &format!("/versions/{version}"), &[])?,
            t.as_deref(),
        )?;
        if value.pointer("/version/version").and_then(Value::as_str) != Some(version)
            || value.pointer("/skill/slug").and_then(Value::as_str) != Some(slug)
        {
            return fail(
                "SOURCE_VERSION_UNAVAILABLE",
                "Version detail identity mismatch",
            );
        }
        Some(value)
    } else {
        None
    };
    let status = detail
        .as_ref()
        .and_then(|d| d.pointer("/version/security/status"))
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/moderation/verdict").and_then(Value::as_str))
        .unwrap_or_else(|| {
            if data
                .pointer("/moderation/isSuspicious")
                .and_then(Value::as_bool)
                == Some(true)
            {
                "suspicious"
            } else {
                "unknown"
            }
        });
    if matches!(status, "malicious" | "failed" | "error" | "pending") {
        return fail("SOURCE_BLOCKED", format!("ClawHub scan status: {status}"));
    }
    let params = if let Some(v) = &candidate.version {
        vec![("version", v.as_str())]
    } else {
        vec![("tag", candidate.selector.as_str())]
    };
    let response = http.get(&endpoint(source, "download", &params)?, t.as_deref())?;
    let mut evidence = Evidence {
        scan: Some(Scan {
            status: status.into(),
            observed_at: env::now(),
        }),
        ..Evidence::default()
    };
    if response.content_type.contains("json") {
        let descriptor: Value = serde_json::from_slice(&response.bytes)
            .map_err(|_| Error::new("PROTOCOL", "Malformed handoff JSON", 2))?;
        if required(&descriptor, "sourceRef")? != "public-github" {
            return fail("PROTOCOL", "Unknown download descriptor");
        }
        // The current handoff does not bind itself to a historical published version.
        if candidate.version.is_some() {
            return fail(
                "SOURCE_VERSION_UNAVAILABLE",
                "Current GitHub snapshot cannot prove a historical release",
            );
        }
        let handoff = Handoff {
            repo: required(&descriptor, "repo")?.into(),
            commit: required(&descriptor, "commit")?.into(),
            path: required(&descriptor, "path")?.into(),
            content_hash: required(&descriptor, "contentHash")?.to_ascii_lowercase(),
        };
        let repo_parts: Vec<_> = handoff.repo.split('/').collect();
        if repo_parts.len() != 2
            || !repo_parts.iter().all(|s| safe_name(s))
            || !is_hex(&handoff.commit, 40)
            || !is_hex(&handoff.content_hash, 64)
        {
            return fail("PROTOCOL", "Invalid GitHub source identity");
        }
        paths::relative(&handoff.path)?;
        if candidate
            .revision
            .as_ref()
            .is_some_and(|rev| *rev != handoff.commit)
        {
            return fail(
                "SOURCE_VERSION_UNAVAILABLE",
                "Current GitHub snapshot differs from locked revision",
            );
        }
        let url = required(&descriptor, "archiveUrl")?;
        let expected = format!(
            "https://api.github.com/repos/{}/zipball/{}",
            handoff.repo, handoff.commit
        );
        if url != expected {
            return fail(
                "PROTOCOL",
                "Handoff archive URL is not the fixed GitHub commit URL",
            );
        }
        let bytes = http.get(url, None)?.bytes;
        evidence.archive_sha256 = Some(digest(&bytes));
        store::extract(&bytes, dest)?;
        let roots = std::fs::read_dir(dest)?.collect::<std::io::Result<Vec<_>>>()?;
        if roots.len() != 1 || !roots[0].file_type()?.is_dir() {
            return fail(
                "PROTOCOL",
                "GitHub archive must have exactly one repository root",
            );
        }
        let root = roots[0].path().join(paths::relative(&handoff.path)?);
        let files = store::inventory(&root)?;
        let actual = github_hash(&files)?;
        if actual != handoff.content_hash {
            return fail(
                "CHECKSUM_MISMATCH",
                "GitHub folder contentHash differs from ClawHub",
            );
        }
        evidence.upstream_hash = Some(actual);
        evidence.upstream_hash_algorithm = Some("clawhub-github-folder-sha256".into());
        let mut selected = candidate.clone();
        selected.revision = Some(handoff.commit.clone());
        evidence.handoff = Some(handoff);
        return Ok((root, selected, evidence));
    }
    if !(response.content_type.contains("zip") || response.content_type.contains("octet-stream"))
        || !response.bytes.starts_with(b"PK")
    {
        return fail(
            "PROTOCOL",
            "Unknown download response; expected ZIP or public-github JSON",
        );
    }
    if candidate.version.is_none() {
        return fail(
            "SOURCE_VERSION_UNAVAILABLE",
            "ZIP download has no verified release version",
        );
    }
    evidence.archive_sha256 = Some(digest(&response.bytes));
    store::extract(&response.bytes, dest)?;
    if let Some(files) = detail.as_ref().and_then(|d| d.pointer("/version/files")) {
        let entries = files
            .as_array()
            .ok_or_else(|| Error::new("PROTOCOL", "Version files is not an array", 2))?;
        let actual = store::inventory(dest)?;
        let mut seen = BTreeSet::new();
        for file in entries {
            let path = required(file, "path")?;
            paths::relative(path)?;
            if !seen.insert(path) {
                return fail("PROTOCOL", "Duplicate upstream file record");
            }
            let expected = required(file, "sha256")?;
            let record = actual
                .iter()
                .find(|f| f.path == path)
                .ok_or_else(|| Error::new("CHECKSUM_MISMATCH", "Upstream file missing", 1))?;
            if expected.to_ascii_lowercase() != record.sha256
                || file
                    .get("size")
                    .and_then(Value::as_u64)
                    .is_some_and(|s| s != record.size)
            {
                return fail("CHECKSUM_MISMATCH", "Upstream file checksum differs");
            }
        }
        if entries.len() != actual.len() {
            return fail(
                "CHECKSUM_MISMATCH",
                "Downloaded file inventory differs from release",
            );
        }
        evidence.upstream_hash = Some(json_digest(files)?);
        evidence.upstream_hash_algorithm = Some("sha256-of-upstream-file-manifest".into());
    }
    Ok((dest.to_path_buf(), candidate.clone(), evidence))
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("PROTOCOL", format!("Missing string field {key}"), 2))
}
pub fn github_hash(files: &[FileRecord]) -> Result<String> {
    let collator = icu_collator::Collator::try_new(
        icu_locale::locale!("en-US").into(),
        icu_collator::options::CollatorOptions::default(),
    )
    .map_err(|_| {
        Error::new(
            "PROTOCOL",
            "Cannot initialize upstream filename collation",
            2,
        )
    })?;
    let mut files: Vec<_> = files.iter().collect();
    files.sort_by(|a, b| collator.compare(&a.path, &b.path));
    let payload = files
        .iter()
        .map(|f| format!("{}\0{}\0{}", f.path, f.size, f.sha256.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(digest(payload.as_bytes()))
}
