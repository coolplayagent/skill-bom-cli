//! Serializable contracts and pure validation. No environment or I/O access.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;
pub const SCHEMA: u32 = 1;
pub const RESOLVER: &str = "1";
pub const MAX_PACKAGES: usize = 256;
pub const MAX_CANDIDATES: usize = 2048;
pub const MAX_ATTEMPTS: usize = 10000;
pub const MAX_FILES: usize = 4096;
pub const MAX_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub code: String,
    pub message: Box<str>,
    pub phase: Box<str>,
    pub package: Option<Box<str>>,
    pub chains: Vec<Vec<String>>,
    pub hint: Box<str>,
    pub exit_code: u8,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            code: code.into(),
            message: message.into().into(),
            phase: "validation".into(),
            package: None,
            chains: vec![],
            hint: "Correct the reported input and retry.".into(),
            exit_code,
        }
    }
    pub fn phase(mut self, phase: &str) -> Self {
        self.phase = phase.into();
        self
    }
    pub fn package(mut self, id: &str) -> Self {
        self.package = Some(id.into());
        self
    }
    pub fn hint(mut self, hint: &str) -> Self {
        self.hint = hint.into();
        self
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO", e.to_string(), 2).phase("filesystem")
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new("JSON", e.to_string(), 2)
    }
}
pub fn fail<T>(code: &str, message: impl Into<String>) -> Result<T> {
    Err(Error::new(code, message, 1))
}
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}
pub fn json_digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(digest(&serde_json::to_vec(value)?))
}
pub fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && !s.starts_with('.')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && ![
            "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ]
        .contains(
            &s.split('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
        )
        && !s.ends_with('.')
}

pub fn validate_relative(s: &str) -> Result<()> {
    if s.is_empty() || s.contains('\\') || s.contains(':') || s.starts_with('/') || s.contains('\0')
    {
        return fail("UNSAFE_PATH", format!("Invalid relative path {s:?}"));
    }
    for part in s.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.eq_ignore_ascii_case(".skill-bom")
            || part.eq_ignore_ascii_case(".git")
            || part.ends_with([' ', '.'])
            || part
                .chars()
                .any(|c| c.is_control() || "<>\"|?*".contains(c))
        {
            return fail("UNSAFE_PATH", format!("Unsafe path {s:?}"));
        }
        let base = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        if [
            "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ]
        .contains(&base.as_str())
        {
            return fail("UNSAFE_PATH", "Reserved device name");
        }
    }
    Ok(())
}
pub fn tree_digest(files: &[FileRecord]) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"skill-tree-sha256-v1\0");
    for f in files {
        hash.update((f.path.len() as u64).to_be_bytes());
        hash.update(f.path.as_bytes());
        hash.update(f.size.to_be_bytes());
        hash.update(
            hex::decode(&f.sha256)
                .map_err(|_| Error::new("LOCK_INVALID", "Invalid file SHA-256", 1))?,
        );
    }
    Ok(hex::encode(hash.finalize()))
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub version: Option<String>,
    pub rev: Option<String>,
    pub tag: Option<String>,
    pub registry: Option<String>,
    pub package: Option<Box<str>>,
    pub git: Option<String>,
    pub url: Option<String>,
    pub subdir: Option<String>,
    pub tag_pattern: Option<String>,
    pub sha256: Option<String>,
}
impl Dependency {
    pub fn request(&self) -> String {
        self.version
            .clone()
            .or_else(|| self.rev.as_ref().map(|v| format!("rev:{v}")))
            .or_else(|| self.tag.as_ref().map(|v| format!("tag:{v}")))
            .unwrap_or_default()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Git {
        repository: String,
        subdir: String,
    },
    Archive {
        url: String,
    },
    Clawhub {
        registry: String,
        owner: String,
        slug: String,
    },
}
impl Source {
    pub fn id(&self) -> String {
        match self {
            Self::Git { repository, subdir } => format!("git:{repository}#{subdir}"),
            Self::Archive { url } => format!("archive:{url}"),
            Self::Clawhub {
                registry,
                owner,
                slug,
            } => format!("clawhub:{registry}/@{owner}/{slug}"),
        }
    }
    pub fn location(&self) -> &str {
        match self {
            Self::Git { repository, .. } => repository,
            Self::Archive { url } => url,
            Self::Clawhub { registry, .. } => registry,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub version: Option<String>,
    pub revision: Option<String>,
    pub selector: String,
}
impl Candidate {
    pub fn display(&self) -> String {
        self.version
            .clone()
            .or_else(|| self.revision.clone())
            .unwrap_or_else(|| self.selector.clone())
    }
    pub fn matches(&self, d: &Dependency) -> bool {
        if let Some(v) = &d.version {
            return self
                .version
                .as_ref()
                .and_then(|s| semver::Version::parse(s).ok())
                .zip(semver::VersionReq::parse(v).ok())
                .is_some_and(|(v, r)| r.matches(&v));
        }
        d.rev
            .as_ref()
            .or(d.tag.as_ref())
            .is_some_and(|s| self.selector == *s || self.revision.as_ref() == Some(s))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub license: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub schema_version: u32,
    pub package: PackageInfo,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetadataKind {
    Upstream,
    UserComplete,
    UserPartial,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub name: String,
    pub license: Option<String>,
    pub description: Option<String>,
    pub dependency_metadata: MetadataKind,
    pub metadata_digest: Option<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}
impl Metadata {
    pub fn complete(&self) -> bool {
        matches!(
            self.dependency_metadata,
            MetadataKind::Upstream | MetadataKind::UserComplete
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub executable: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub archive_sha256: Option<String>,
    pub upstream_hash: Option<String>,
    pub upstream_hash_algorithm: Option<String>,
    pub handoff: Option<Handoff>,
    pub scan: Option<Scan>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    pub repo: String,
    pub commit: String,
    pub path: String,
    pub content_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scan {
    pub status: String,
    pub observed_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub package: String,
    pub request: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    pub source: Source,
    pub candidate: Candidate,
    pub metadata: Metadata,
    pub tree_sha256: String,
    pub tree_algorithm: String,
    pub files: Vec<FileRecord>,
    pub evidence: Evidence,
    pub dependencies: BTreeMap<String, Edge>,
    pub directory: String,
    pub acquisition: Dependency,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    pub schema_version: u32,
    pub resolver_version: String,
    pub manifest_digest: String,
    pub roots: BTreeMap<String, Edge>,
    pub packages: BTreeMap<String, LockedPackage>,
}
impl Lock {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA || self.resolver_version != RESOLVER {
            return fail("SCHEMA_VERSION", "Unsupported lock or resolver version");
        }
        if self.packages.len() > MAX_PACKAGES {
            return fail("RESOURCE_LIMIT", "Dependency graph exceeds package limit");
        }
        let mut directories = BTreeSet::new();
        for (id, p) in &self.packages {
            if p.source.id() != *id
                || !safe_name(&p.directory)
                || !is_hex(&p.tree_sha256, 64)
                || p.tree_algorithm != "skill-tree-sha256-v1"
            {
                return fail("LOCK_INVALID", format!("Invalid locked package {id}"));
            }
            if !directories.insert(p.directory.to_ascii_lowercase()) {
                return fail("DIRECTORY_CONFLICT", &p.directory);
            }
            if let Some(rev) = &p.candidate.revision
                && !is_hex(rev, 40)
                && !is_hex(rev, 64)
            {
                return fail("LOCK_INVALID", "Revision must be a full commit");
            }
            if p.candidate.version.is_none() && p.candidate.revision.is_none() {
                return fail("LOCK_INVALID", "Package has neither version nor revision");
            }
            if p.candidate
                .version
                .as_ref()
                .is_some_and(|v| semver::Version::parse(v).is_err())
                || (matches!(p.source, Source::Git { .. }) && p.candidate.revision.is_none())
            {
                return fail("LOCK_INVALID", "Invalid release or missing Git commit");
            }
            let mut last_path = None;
            let mut bytes = 0u64;
            let mut entries = 0;
            if p.files.len() > MAX_FILES {
                return fail("RESOURCE_LIMIT", "Locked file inventory exceeds budget");
            }
            for file in &p.files {
                validate_relative(&file.path)?;
                if !is_hex(&file.sha256, 64)
                    || last_path.is_some_and(|last| last >= file.path.as_str())
                {
                    return fail(
                        "LOCK_INVALID",
                        "File inventory must have valid hashes and unique sorted paths",
                    );
                }
                last_path = Some(file.path.as_str());
                bytes = bytes
                    .checked_add(file.size)
                    .ok_or_else(|| Error::new("RESOURCE_LIMIT", "File size overflow", 2))?;
                if ["SKILL.md", "skill.md", "skills.md"].contains(&file.path.as_str()) {
                    entries += 1;
                }
            }
            if bytes > MAX_BYTES {
                return fail("RESOURCE_LIMIT", "Locked content exceeds byte budget");
            }
            if entries != 1 || tree_digest(&p.files)? != p.tree_sha256 {
                return fail(
                    "LOCK_INVALID",
                    "Entrypoint or content tree digest differs from inventory",
                );
            }
            for e in p.dependencies.values() {
                if !self.packages.contains_key(&e.package) {
                    return fail("LOCK_INVALID", "Missing dependency node");
                }
            }
        }
        let mut visited = BTreeSet::new();
        for e in self.roots.values() {
            self.visit(&e.package, &mut vec![], &mut visited)?;
        }
        if visited.len() != self.packages.len() {
            return fail("LOCK_INVALID", "Unreachable lock node");
        }
        Ok(())
    }
    fn visit(&self, id: &str, stack: &mut Vec<String>, seen: &mut BTreeSet<String>) -> Result<()> {
        if stack.iter().any(|s| s == id) {
            let mut e = Error::new(
                "DEPENDENCY_CYCLE",
                format!("{} -> {id}", stack.join(" -> ")),
                1,
            );
            e.chains.push(stack.clone());
            return Err(e);
        }
        if seen.contains(id) {
            return Ok(());
        }
        let p = self
            .packages
            .get(id)
            .ok_or_else(|| Error::new("LOCK_INVALID", format!("Missing node {id}"), 1))?;
        stack.push(id.into());
        for e in p.dependencies.values() {
            self.visit(&e.package, stack, seen)?;
        }
        stack.pop();
        seen.insert(id.into());
        Ok(())
    }
    pub fn paths_to(&self, target: &str) -> Result<Vec<Vec<String>>> {
        fn walk(
            lock: &Lock,
            id: &str,
            target: &str,
            path: &mut Vec<String>,
            out: &mut Vec<Vec<String>>,
            count: &mut usize,
        ) -> Result<()> {
            *count += 1;
            if *count > MAX_ATTEMPTS || path.len() > MAX_PACKAGES {
                return Err(Error::new(
                    "RESOURCE_LIMIT",
                    "Dependency explanation exceeds path budget",
                    2,
                ));
            }
            if path.iter().any(|s| s == id) {
                return fail("DEPENDENCY_CYCLE", "Cycle in explanation graph");
            }
            path.push(id.into());
            if id == target {
                out.push(path.clone());
            } else if let Some(p) = lock.packages.get(id) {
                for edge in p.dependencies.values() {
                    walk(lock, &edge.package, target, path, out, count)?;
                }
            }
            path.pop();
            Ok(())
        }
        let mut out = vec![];
        let mut count = 0;
        for (alias, edge) in &self.roots {
            walk(
                self,
                &edge.package,
                target,
                &mut vec![alias.clone()],
                &mut out,
                &mut count,
            )?;
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "domain_tests.rs"]
mod tests;
