//! Source adapters implement the resolver's I/O-free provider contract.
pub use crate::agentcenter;
pub use crate::archive;
pub use crate::clawhub;
use crate::domain::*;
pub use crate::git;
use crate::{auth, config::Manifest, net::Http, paths, store};
use std::collections::BTreeMap;
use std::path::Path;

pub trait SourceProvider {
    fn candidates(&mut self, source: &Source, request: &Dependency) -> Result<Vec<Candidate>>;
    fn materialize(
        &mut self,
        source: &Source,
        candidate: &Candidate,
        request: &Dependency,
        previous: Option<&LockedPackage>,
    ) -> Result<LockedPackage>;
}
pub struct Provider<'a> {
    pub manifest: &'a Manifest,
    pub store: store::Store,
    pub http: Http,
    pub strict: bool,
    pub credentials: Box<dyn auth::CredentialProvider + 'a>,
    catalogs: BTreeMap<String, Vec<Candidate>>,
}
impl<'a> Provider<'a> {
    pub fn new(
        manifest: &'a Manifest,
        cache: std::path::PathBuf,
        offline: bool,
        strict: bool,
    ) -> Result<Self> {
        Ok(Self {
            manifest,
            store: store::Store::new(cache),
            http: Http::new(offline)?,
            strict,
            credentials: Box::new(auth::SystemProvider),
            catalogs: BTreeMap::new(),
        })
    }
    pub fn ensure(&mut self, p: &LockedPackage) -> Result<std::path::PathBuf> {
        let root = match self.store.get(p) {
            Ok(path) => Ok(path),
            Err(e) if self.http.offline => Err(e),
            Err(_) => {
                let restored =
                    self.materialize(&p.source, &p.candidate, &p.acquisition, Some(p))?;
                if restored.tree_sha256 != p.tree_sha256
                    || !store::same_files(&restored.files, &p.files)
                    || restored.evidence.archive_sha256 != p.evidence.archive_sha256
                {
                    return fail(
                        "CONTENT_CHANGED",
                        "Remote content differs from the locked content",
                    );
                }
                self.store.get(p)
            }
        }?;
        store::validate_skill(&root, p)?;
        Ok(root)
    }
}
impl SourceProvider for Provider<'_> {
    fn candidates(&mut self, source: &Source, request: &Dependency) -> Result<Vec<Candidate>> {
        if self.http.offline {
            return Err(Error::new(
                "OFFLINE_MISS",
                "No cached candidate satisfies this request",
                2,
            ));
        }
        let key = json_digest(&(source, request))?;
        if let Some(c) = self.catalogs.get(&key) {
            return Ok(c.clone());
        }
        let candidates = match source {
            Source::Archive { .. } => vec![Candidate {
                version: Some(
                    request
                        .version
                        .as_deref()
                        .unwrap_or("")
                        .trim_start_matches('=')
                        .into(),
                ),
                revision: None,
                selector: request.request(),
            }],
            Source::Git { repository, .. } => git::candidates(repository, request)?,
            Source::Clawhub { .. } => clawhub::candidates_with_auth(
                &self.http,
                self.credentials.as_ref(),
                self.manifest,
                source,
                request,
            )?,
            Source::Agentcenter { .. } => agentcenter::candidates_with_auth(
                &self.http,
                self.credentials.as_ref(),
                self.manifest,
                source,
                request,
            )?,
        };
        if candidates.len() > MAX_CANDIDATES {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Too many candidate versions",
                2,
            ));
        }
        self.catalogs.insert(key, candidates.clone());
        Ok(candidates)
    }
    fn materialize(
        &mut self,
        source: &Source,
        candidate: &Candidate,
        request: &Dependency,
        previous: Option<&LockedPackage>,
    ) -> Result<LockedPackage> {
        if let Some(p) = previous
            && p.candidate == *candidate
            && p.source == *source
            && let Ok(root) = self.store.get(p)
        {
            let mut p = p.clone();
            p.metadata = metadata(self.manifest, source, candidate, &root, self.strict)?;
            p.directory = p.metadata.name.clone();
            p.acquisition = request.clone();
            return Ok(p);
        }
        if self.http.offline {
            return Err(
                Error::new("OFFLINE_MISS", "Package content is missing or corrupt", 2)
                    .package(&source.id()),
            );
        }
        let temp = tempfile::tempdir()?;
        let (root, actual, evidence) = match source {
            Source::Archive { url } => {
                let (root, evidence) = archive::fetch(&self.http, url, request, temp.path())?;
                (root, candidate.clone(), evidence)
            }
            Source::Git { repository, subdir } => {
                git::fetch(repository, subdir, candidate, temp.path())?;
                (
                    temp.path().to_path_buf(),
                    candidate.clone(),
                    Evidence::default(),
                )
            }
            Source::Clawhub { .. } => clawhub::fetch_with_auth(
                &self.http,
                self.credentials.as_ref(),
                self.manifest,
                source,
                candidate,
                temp.path(),
            )?,
            Source::Agentcenter { .. } => agentcenter::fetch_with_auth(
                &self.http,
                self.credentials.as_ref(),
                self.manifest,
                source,
                candidate,
                previous
                    .filter(|p| p.candidate == *candidate)
                    .and_then(|p| p.evidence.archive_sha256.as_deref()),
                temp.path(),
            )?,
        };
        let meta = metadata(self.manifest, source, &actual, &root, self.strict)?;
        let (hash, files) = self.store.publish(&root)?;
        let p = LockedPackage {
            source: source.clone(),
            candidate: actual,
            directory: meta.name.clone(),
            metadata: meta,
            tree_sha256: hash,
            tree_algorithm: "skill-tree-sha256-v1".into(),
            files,
            evidence,
            dependencies: BTreeMap::new(),
            acquisition: request.clone(),
        };
        if let Some(old) = previous
            && old.candidate == p.candidate
            && (old.tree_sha256 != p.tree_sha256
                || old.evidence.archive_sha256 != p.evidence.archive_sha256)
        {
            return fail(
                "CONTENT_CHANGED",
                "Immutable source changed since previous lock",
            );
        }
        Ok(p)
    }
}
pub fn metadata(
    manifest: &Manifest,
    source: &Source,
    candidate: &Candidate,
    root: &Path,
    strict: bool,
) -> Result<Metadata> {
    let skill = store::skill(root)?;
    let path = root.join("skill.toml");
    let supplements: Vec<_> = manifest
        .package_metadata
        .iter()
        .filter(|s| {
            manifest.source(&s.source).is_ok_and(|src| src == *source)
                && candidate.matches(&s.source)
        })
        .collect();
    if supplements.len() > 1 {
        return fail("METADATA", "Multiple matching supplements");
    }
    let mut meta = if path.exists() {
        if !supplements.is_empty() {
            return fail(
                "METADATA_OVERRIDE",
                "User metadata cannot override upstream skill.toml",
            );
        }
        let bytes = paths::read(&path, 1024 * 1024)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::new("METADATA", "skill.toml is not UTF-8", 2))?;
        let m: PackageManifest =
            toml::from_str(text).map_err(|e| Error::new("METADATA", e.to_string(), 2))?;
        if m.schema_version != SCHEMA {
            return fail("METADATA", "Invalid package schema or name");
        }
        skill.matches_directory(&skill_name(&m.package.name)?)?;
        semver::Version::parse(&m.package.version)
            .map_err(|_| Error::new("METADATA", "Package version is not SemVer", 1))?;
        if candidate
            .version
            .as_ref()
            .is_some_and(|v| *v != m.package.version)
        {
            return fail(
                "VERSION_MISMATCH",
                "skill.toml version differs from selected release",
            );
        }
        manifest.validate_deps(&m.dependencies)?;
        Metadata {
            name: skill.name.clone(),
            license: m.package.license.or_else(|| skill.license.clone()),
            description: Some(skill.description.clone()),
            dependency_metadata: MetadataKind::Upstream,
            metadata_digest: Some(digest(&bytes)),
            dependencies: m.dependencies,
            diagnostics: vec![],
        }
    } else if let Some(s) = supplements.first() {
        skill.matches_directory(&skill_name(&s.name)?)?;
        Metadata {
            name: skill.name.clone(),
            license: skill.license.clone(),
            description: Some(skill.description.clone()),
            dependency_metadata: if s.complete {
                MetadataKind::UserComplete
            } else {
                MetadataKind::UserPartial
            },
            metadata_digest: Some(json_digest(s)?),
            dependencies: s.dependencies.clone(),
            diagnostics: vec![
                "Dependency metadata supplied by the root manifest maintainer".into(),
            ],
        }
    } else {
        Metadata {
            name: skill.name.clone(),
            license: skill.license.clone(),
            description: Some(skill.description.clone()),
            dependency_metadata: MetadataKind::Unknown,
            metadata_digest: None,
            dependencies: BTreeMap::new(),
            diagnostics: vec![
                "Skill dependency metadata is unknown; runtime tools are not installed".into(),
            ],
        }
    };
    if let Some(v) = skill
        .extensions
        .get("version")
        .and_then(serde_json::Value::as_str)
        && candidate
            .version
            .as_ref()
            .is_some_and(|selected| selected != v)
    {
        meta.diagnostics.push(format!(
            "SKILL.md auxiliary version {v} differs from selected release"
        ));
    }
    if strict && !meta.complete() {
        return fail(
            "METADATA_UNKNOWN",
            "Strict metadata requires a complete dependency declaration",
        );
    }
    Ok(meta)
}
