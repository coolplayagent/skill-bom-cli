use crate::domain::*;
use crate::{paths, process, store};
use std::collections::BTreeMap;
use std::path::Path;

pub fn candidates(repository: &str, request: &Dependency) -> Result<Vec<Candidate>> {
    if let Some(rev) = &request.rev {
        let revision = if is_hex(rev, 40) || is_hex(rev, 64) {
            rev.to_ascii_lowercase()
        } else {
            let output = process::git(
                None,
                &[
                    "ls-remote",
                    "--",
                    repository,
                    rev,
                    &format!("refs/heads/{rev}"),
                    &format!("refs/tags/{rev}"),
                    &format!("refs/tags/{rev}^{{}}"),
                ],
            )?;
            let text = String::from_utf8(output)
                .map_err(|_| Error::new("GIT_PROTOCOL", "Git refs are not UTF-8", 2))?;
            let refs: Vec<_> = text.lines().filter_map(|l| l.split_once('\t')).collect();
            let peeled: Vec<_> = refs.iter().filter(|(_, r)| r.ends_with("^{}")).collect();
            if let Some((c, _)) = peeled.first() {
                c.to_string()
            } else if refs.len() == 1 {
                refs[0].0.into()
            } else {
                return fail("REVISION_AMBIGUOUS", "Git revision is missing or ambiguous");
            }
        };
        return Ok(vec![Candidate {
            version: None,
            revision: Some(revision),
            selector: rev.clone(),
        }]);
    }
    let output = process::git(None, &["ls-remote", "--tags", "--", repository])?;
    let text = String::from_utf8(output)
        .map_err(|_| Error::new("GIT_PROTOCOL", "Git refs are not UTF-8", 2))?;
    let mut refs = BTreeMap::new();
    let mut peeled = BTreeMap::new();
    for line in text.lines() {
        let (commit, tag) = line
            .split_once('\t')
            .ok_or_else(|| Error::new("GIT_PROTOCOL", "Malformed ls-remote output", 2))?;
        if !is_hex(commit, 40) && !is_hex(commit, 64) {
            return fail("GIT_PROTOCOL", "Invalid Git object id");
        }
        if let Some(t) = tag.strip_prefix("refs/tags/") {
            if let Some(t) = t.strip_suffix("^{}") {
                peeled.insert(t.to_string(), commit.to_string());
            } else {
                refs.insert(t.to_string(), commit.to_string());
            }
        }
        if refs.len() > MAX_CANDIDATES {
            return Err(Error::new("RESOURCE_LIMIT", "Too many Git tags", 2));
        }
    }
    refs.extend(peeled);
    let pattern = request.tag_pattern.as_deref().unwrap_or("v{version}");
    let (prefix, suffix) = pattern
        .split_once("{version}")
        .ok_or_else(|| Error::new("TAG_PATTERN", "Missing {version}", 2))?;
    Ok(refs
        .into_iter()
        .filter_map(|(tag, commit)| {
            let raw = tag.strip_prefix(prefix)?.strip_suffix(suffix)?;
            let v = semver::Version::parse(raw).ok()?;
            Some(Candidate {
                version: Some(v.to_string()),
                revision: Some(commit),
                selector: tag,
            })
        })
        .collect())
}
pub fn fetch(repository: &str, subdir: &str, candidate: &Candidate, dest: &Path) -> Result<()> {
    let commit = candidate
        .revision
        .as_deref()
        .ok_or_else(|| Error::new("REVISION", "Git requires immutable commit", 1))?;
    if !is_hex(commit, 40) && !is_hex(commit, 64) {
        return fail("REVISION", "Invalid commit");
    }
    if !subdir.is_empty() {
        paths::relative(subdir)?;
    }
    let repo = tempfile::tempdir()?;
    process::git(Some(repo.path()), &["init", "--bare", "--quiet"])?;
    process::git(
        Some(repo.path()),
        &[
            "fetch",
            "--quiet",
            "--depth=1",
            "--no-recurse-submodules",
            "--",
            repository,
            commit,
        ],
    )?;
    let resolved = process::git(
        Some(repo.path()),
        &["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
    )?;
    if String::from_utf8_lossy(&resolved).trim() != commit {
        return fail("REVISION", "Fetched commit differs from lock");
    }
    let tree = if subdir.is_empty() {
        commit.into()
    } else {
        format!("{commit}:{subdir}")
    };
    let entries = process::git(Some(repo.path()), &["ls-tree", "-rz", &tree])?;
    for entry in entries.split(|b| *b == 0) {
        if entry.starts_with(b"160000 ") {
            return fail(
                "SUBMODULE_UNMATERIALIZED",
                "Skill contains an unmaterialized submodule",
            );
        }
    }
    let archive = process::git(Some(repo.path()), &["archive", "--format=tar", &tree])?;
    store::extract(&archive, dest)
}
