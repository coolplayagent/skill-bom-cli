use crate::domain::*;
use crate::{env, paths};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub target: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub kind: String,
    pub url: String,
    pub token_env: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Supplement {
    pub source: Dependency,
    pub name: String,
    pub complete: bool,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub project: Project,
    #[serde(default)]
    pub install: Install,
    #[serde(default)]
    pub registries: BTreeMap<String, Registry>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub package_metadata: Vec<Supplement>,
}
impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let m: Self = toml::from_str(text).map_err(|e| Error::new("CONFIG", e.to_string(), 2))?;
        m.validate().map_err(|mut e| {
            e.exit_code = 2;
            e
        })?;
        Ok(m)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA {
            return fail("SCHEMA_VERSION", "Unsupported manifest schema");
        }
        if !safe_name(&self.project.name) {
            return fail("CONFIG", "Invalid project name");
        }
        for (alias, r) in &self.registries {
            if !safe_name(alias) || !matches!(r.kind.as_str(), "clawhub" | "agentcenter") {
                return fail(
                    "REGISTRY",
                    "Only named ClawHub or AgentCenter registries are supported",
                );
            }
            let registry_url = web_url(&r.url)?;
            if r.kind == "agentcenter" && r.token_env.is_none() {
                return fail(
                    "REGISTRY",
                    "AgentCenter requires token_env for X-Auth-Token",
                );
            }
            if r.kind == "agentcenter"
                && url::Url::parse(&registry_url).is_ok_and(|url| url.path() != "/")
            {
                return fail(
                    "REGISTRY",
                    "AgentCenter Registry URL must be an origin without a path",
                );
            }
            if let Some(s) = &r.token_env
                && (s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            {
                return fail("REGISTRY", "Invalid token environment variable name");
            }
        }
        self.validate_deps(&self.dependencies)?;
        let mut supplements = std::collections::BTreeSet::new();
        for s in &self.package_metadata {
            let id = self.source(&s.source)?.id();
            if !safe_name(&s.name)
                || !(s.source.version.as_deref().is_some_and(exact_version)
                    || s.source
                        .rev
                        .as_deref()
                        .is_some_and(|v| is_hex(v, 40) || is_hex(v, 64)))
            {
                return fail(
                    "METADATA",
                    "Supplement requires safe name and exact version or full commit",
                );
            }
            if !supplements.insert((id, s.source.request())) {
                return fail("METADATA", "Duplicate supplement");
            }
            self.validate_deps(&s.dependencies)?;
        }
        Ok(())
    }
    pub fn validate_deps(&self, deps: &BTreeMap<String, Dependency>) -> Result<()> {
        if deps.len() > MAX_PACKAGES {
            return Err(Error::new("RESOURCE_LIMIT", "Too many dependencies", 2));
        }
        for (a, d) in deps {
            if !safe_name(a) {
                return fail("CONFIG", "Invalid dependency alias");
            }
            self.source(d)?;
        }
        Ok(())
    }
    pub fn source(&self, d: &Dependency) -> Result<Source> {
        if [d.git.is_some(), d.url.is_some(), d.registry.is_some()]
            .into_iter()
            .filter(|v| *v)
            .count()
            != 1
        {
            return fail(
                "SOURCE_COMBINATION",
                "Specify exactly one of git, url, registry",
            );
        }
        if [d.version.is_some(), d.rev.is_some(), d.tag.is_some()]
            .into_iter()
            .filter(|v| *v)
            .count()
            != 1
        {
            return fail(
                "SOURCE_COMBINATION",
                "Specify exactly one of version, rev, tag",
            );
        }
        if let Some(v) = &d.version {
            semver::VersionReq::parse(v).map_err(|e| Error::new("VERSION", e.to_string(), 2))?;
        }
        if let Some(s) = &d.subdir {
            paths::relative(s)?;
        }
        if let Some(g) = &d.git {
            if d.package.is_some() || d.sha256.is_some() || d.tag.is_some() {
                return fail(
                    "SOURCE_COMBINATION",
                    "Git accepts version/tag_pattern or rev, plus subdir",
                );
            }
            if let Some(p) = &d.tag_pattern
                && (d.version.is_none()
                    || p.matches("{version}").count() != 1
                    || p.starts_with('-'))
            {
                return fail("TAG_PATTERN", "Pattern must contain one {version}");
            }
            if d.rev.as_deref().is_some_and(|s| {
                s.is_empty() || s.starts_with('-') || s.chars().any(char::is_control)
            }) {
                return fail("REVISION", "Invalid Git revision");
            }
            return Ok(Source::Git {
                repository: git_url(g)?,
                subdir: d.subdir.clone().unwrap_or_default(),
            });
        }
        if let Some(u) = &d.url {
            if d.package.is_some()
                || d.rev.is_some()
                || d.tag.is_some()
                || d.tag_pattern.is_some()
                || !d.version.as_deref().is_some_and(exact_version)
                || !d.sha256.as_deref().is_some_and(|v| is_hex(v, 64))
            {
                return fail("ARCHIVE_CONFIG", "Archive requires =x.y.z and full sha256");
            }
            return Ok(Source::Archive { url: web_url(u)? });
        }
        if d.rev.is_some() || d.sha256.is_some() || d.tag_pattern.is_some() {
            return fail(
                "SOURCE_COMBINATION",
                "Registry accepts package and version or tag",
            );
        }
        let r = self
            .registries
            .get(d.registry.as_deref().unwrap_or(""))
            .ok_or_else(|| Error::new("REGISTRY", "Registry alias must be defined at root", 2))?;
        let p = d.package.as_deref().unwrap_or("");
        if r.kind == "agentcenter" {
            if !safe_name(p) || d.tag.is_some() || d.version.is_none() {
                return fail(
                    "PACKAGE_ID",
                    "AgentCenter requires a stable skillId and a SemVer version constraint",
                );
            }
            return Ok(Source::Agentcenter {
                registry: web_url(&r.url)?,
                skill_id: p.to_string(),
                subdir: d.subdir.clone().unwrap_or_default(),
            });
        }
        if d.subdir.is_some() {
            return fail("SOURCE_COMBINATION", "ClawHub does not accept subdir");
        }
        let (owner, slug) = p
            .strip_prefix('@')
            .and_then(|s| s.split_once('/'))
            .ok_or_else(|| Error::new("PACKAGE_ID", "ClawHub package must be @owner/slug", 2))?;
        if !safe_name(owner) || !safe_name(slug) || d.tag.as_deref().is_some_and(|s| !safe_name(s))
        {
            return fail("PACKAGE_ID", "Invalid owner, slug or tag");
        }
        Ok(Source::Clawhub {
            registry: web_url(&r.url)?,
            owner: owner.to_ascii_lowercase(),
            slug: slug.to_ascii_lowercase(),
        })
    }
    pub fn digest(&self) -> Result<String> {
        json_digest(self)
    }
}
pub fn exact_version(s: &str) -> bool {
    s.strip_prefix('=')
        .is_some_and(|v| semver::Version::parse(v).is_ok())
}
pub fn web_url(s: &str) -> Result<String> {
    let mut u = url::Url::parse(s).map_err(|_| Error::new("URL", "Invalid URL", 2))?;
    let loopback = matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (u.scheme() != "https" && !(u.scheme() == "http" && loopback))
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return fail(
            "URL",
            "Use a credential-free HTTPS URL without query/fragment (loopback HTTP is allowed for tests)",
        );
    }
    u.set_fragment(None);
    Ok(u.to_string().trim_end_matches('/').into())
}
pub fn git_url(s: &str) -> Result<String> {
    if s.starts_with("https://") {
        return Ok(web_url(s)?.trim_end_matches(".git").into());
    }
    if s.starts_with("ssh://") {
        let u = url::Url::parse(s).map_err(|_| Error::new("URL", "Invalid SSH URL", 2))?;
        if u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || u.host_str().is_none()
            || (!u.username().is_empty() && u.username() != "git")
        {
            return fail(
                "URL",
                "SSH URLs must use the conventional git user and contain no credentials",
            );
        }
        return Ok(u
            .to_string()
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .into());
    }
    if let Some(rest) = s.strip_prefix("git@")
        && let Some((host, path)) = rest.split_once(':')
        && !host.is_empty()
        && !path.is_empty()
    {
        return git_url(&format!("ssh://git@{host}/{path}"));
    }
    fail("URL", "Git source must use HTTPS or SSH")
}
#[derive(Debug)]
pub struct Scope {
    pub manifest: PathBuf,
    pub lock: PathBuf,
    pub target: PathBuf,
    pub cache: PathBuf,
    pub owner: String,
}
impl Scope {
    pub fn new(manifest: Option<&Path>, global: bool, target: Option<&Path>) -> Result<Self> {
        if global && manifest.is_some() {
            return fail("SCOPE", "--global conflicts with --manifest");
        }
        let dirs = env::directories()?;
        let manifest = paths::absolute(&manifest.map(Path::to_path_buf).unwrap_or_else(|| {
            if global {
                dirs.config_dir().join("skills.toml")
            } else {
                PathBuf::from("skills.toml")
            }
        }))?;
        let parent = manifest
            .parent()
            .ok_or_else(|| Error::new("PATH", "Manifest needs parent", 2))?;
        let target = match target {
            Some(p) => paths::absolute(p)?,
            None if global => dirs.data_dir().join("skills"),
            None => parent.join("skills"),
        };
        let owner = digest(manifest.to_string_lossy().as_bytes());
        Ok(Self {
            lock: manifest.with_extension("lock"),
            manifest,
            target,
            cache: dirs.cache_dir().join("content-v1"),
            owner,
        })
    }
    pub fn load(&mut self, target_overridden: bool) -> Result<Manifest> {
        let bytes = paths::read(&self.manifest, 1024 * 1024)?;
        let m = Manifest::parse(
            std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("CONFIG", "Manifest must be UTF-8", 2))?,
        )?;
        if !target_overridden && let Some(t) = &m.install.target {
            self.target = paths::absolute(&self.manifest.parent().unwrap().join(t))?;
        }
        Ok(m)
    }
}
pub fn read_lock(path: &Path) -> Result<Lock> {
    let lock: Lock = serde_json::from_slice(&paths::read(path, 16 * 1024 * 1024)?)?;
    lock.validate()?;
    Ok(lock)
}
pub fn write_lock(path: &Path, lock: &Lock) -> Result<()> {
    lock.validate()?;
    let bytes = serde_json::to_vec_pretty(lock)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(Error::new("RESOURCE_LIMIT", "Lock exceeds 16 MiB", 2));
    }
    paths::atomic_write(path, &bytes)
}
