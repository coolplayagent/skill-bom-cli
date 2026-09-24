//! AgentCenter's skillId-addressed HTTP registry. The contract is based on Issue #2.
use crate::domain::*;
use crate::{
    config::{self, Manifest},
    env,
    net::Transport,
    paths, store,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn parts(source: &Source) -> Result<(&str, &str, &str)> {
    if let Source::Agentcenter {
        registry,
        skill_id,
        subdir,
    } = source
    {
        Ok((registry, skill_id, subdir))
    } else {
        fail("SOURCE", "Expected AgentCenter identity")
    }
}

fn token(manifest: &Manifest, registry: &str) -> Result<String> {
    manifest
        .registries
        .values()
        .find(|r| {
            r.kind == "agentcenter" && config::web_url(&r.url).ok().as_deref() == Some(registry)
        })
        .and_then(|r| r.token_env.as_deref())
        .and_then(env::variable)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::new(
                "MISSING_AUTH_TOKEN",
                "AgentCenter X-Auth-Token environment variable is missing",
                2,
            )
            .hint("Set the token_env variable configured for this Registry.")
        })
}

fn endpoint(registry: &str, path: &str, skill_id: Option<&str>) -> Result<String> {
    let mut url = url::Url::parse(registry)
        .map_err(|_| Error::new("URL", "Invalid AgentCenter Registry URL", 2))?;
    url.set_path(path);
    if let Some(id) = skill_id {
        url.query_pairs_mut().append_pair("skillId", id);
    }
    Ok(url.into())
}

fn payload(response: crate::net::Response) -> Result<Value> {
    let value: Value = serde_json::from_slice(&response.bytes)
        .map_err(|_| Error::new("PROTOCOL", "AgentCenter returned invalid JSON", 2))?;
    let code = value.get("code").and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    });
    if matches!(code, Some(40100 | 40300))
        || value.get("success") == Some(&Value::Bool(false)) && matches!(code, Some(401 | 403))
    {
        return fail("AUTH_REQUIRED", "AgentCenter rejected authentication");
    }
    if code.is_some_and(|c| !matches!(c, 0 | 200 | 20000))
        || value.get("success") == Some(&Value::Bool(false))
        || code.is_none() && value.get("success") != Some(&Value::Bool(true))
    {
        return Err(Error::new(
            "PROTOCOL",
            "AgentCenter business response was not successful",
            2,
        ));
    }
    value
        .get("data")
        .filter(|v| v.is_object())
        .cloned()
        .ok_or_else(|| Error::new("PROTOCOL", "AgentCenter response has no data object", 2))
}

fn detail(http: &impl Transport, manifest: &Manifest, source: &Source) -> Result<Value> {
    let (registry, skill_id, _) = parts(source)?;
    let data = payload(http.get_x_auth(
        &endpoint(
            registry,
            "/mcpService/external/skills/v1/get",
            Some(skill_id),
        )?,
        &token(manifest, registry)?,
    )?)?;
    if data.get("skillId").and_then(Value::as_str) != Some(skill_id) {
        return fail(
            "SOURCE_IDENTITY_CHANGED",
            "AgentCenter skillId differs from the declaration",
        );
    }
    Ok(data)
}

pub fn candidates(
    http: &impl Transport,
    manifest: &Manifest,
    source: &Source,
    request: &Dependency,
) -> Result<Vec<Candidate>> {
    let data = detail(http, manifest, source)?;
    let version = data
        .get("latestVersion")
        .or_else(|| data.get("version"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("PROTOCOL", "AgentCenter detail has no latestVersion", 2))?;
    semver::Version::parse(version).map_err(|_| {
        Error::new(
            "SOURCE_VERSION_UNAVAILABLE",
            "AgentCenter latestVersion is not SemVer",
            1,
        )
    })?;
    let candidate = Candidate {
        version: Some(version.into()),
        revision: None,
        selector: version.into(),
    };
    if !candidate.matches(request) {
        return fail(
            "SOURCE_VERSION_UNAVAILABLE",
            "AgentCenter latestVersion does not satisfy the request; historical versions cannot be enumerated",
        );
    }
    Ok(vec![candidate])
}

pub fn fetch(
    http: &impl Transport,
    manifest: &Manifest,
    source: &Source,
    candidate: &Candidate,
    expected_archive: Option<&str>,
    dest: &Path,
) -> Result<(PathBuf, Candidate, Evidence)> {
    let (registry, skill_id, subdir) = parts(source)?;
    let data = detail(http, manifest, source)?;
    let version = candidate.version.as_deref().ok_or_else(|| {
        Error::new(
            "SOURCE_VERSION_UNAVAILABLE",
            "AgentCenter requires a SemVer release",
            1,
        )
    })?;
    if data
        .get("latestVersion")
        .or_else(|| data.get("version"))
        .and_then(Value::as_str)
        != Some(version)
        && expected_archive.is_none()
    {
        return fail(
            "SOURCE_VERSION_UNAVAILABLE",
            "AgentCenter cannot prove this historical version is available",
        );
    }
    let response = http.post_json(
        &endpoint(registry, "/mcpService/external/skills/v1/download", None)?,
        &json!({"skillId":skill_id,"version":version}),
        &token(manifest, registry)?,
    )?;
    if response.content_type.contains("json") {
        payload(response)?;
        return fail(
            "PROTOCOL",
            "AgentCenter download returned JSON instead of ZIP",
        );
    }
    if !response.bytes.starts_with(b"PK")
        || !(response.content_type.contains("zip")
            || response.content_type.contains("octet-stream"))
    {
        return fail("PROTOCOL", "AgentCenter download is not a ZIP archive");
    }
    let hash = digest(&response.bytes);
    if expected_archive.is_some_and(|expected| expected != hash) {
        return fail(
            "CONTENT_CHANGED",
            "AgentCenter archive differs from the locked SHA-256",
        );
    }
    store::extract(&response.bytes, dest)?;
    let root = if subdir.is_empty() {
        dest.to_path_buf()
    } else {
        dest.join(paths::relative(subdir)?)
    };
    store::entrypoint(&root)?;
    Ok((
        root,
        candidate.clone(),
        Evidence {
            archive_sha256: Some(hash),
            ..Evidence::default()
        },
    ))
}
