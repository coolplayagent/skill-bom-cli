//! Deterministic backtracking over standardized candidates. No source-specific I/O.
use crate::config::Manifest;
use crate::domain::*;
use crate::sources::SourceProvider;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Requirement {
    source: Source,
    dependency: Dependency,
    chain: Vec<String>,
}
pub struct Resolver<'a, P> {
    manifest: &'a Manifest,
    provider: &'a mut P,
    previous: Option<&'a Lock>,
    update: BTreeSet<String>,
    attempts: usize,
}
impl<'a, P: SourceProvider> Resolver<'a, P> {
    pub fn new(
        manifest: &'a Manifest,
        provider: &'a mut P,
        previous: Option<&'a Lock>,
        update: BTreeSet<String>,
    ) -> Self {
        Self {
            manifest,
            provider,
            previous,
            update,
            attempts: 0,
        }
    }
    pub fn resolve(&mut self) -> Result<Lock> {
        self.search(BTreeMap::new())
    }
    fn requirements(
        &self,
        selected: &BTreeMap<String, LockedPackage>,
    ) -> Result<BTreeMap<String, Vec<Requirement>>> {
        fn visit(
            m: &Manifest,
            selected: &BTreeMap<String, LockedPackage>,
            deps: &BTreeMap<String, Dependency>,
            path: &[String],
            out: &mut BTreeMap<String, Vec<Requirement>>,
            count: &mut usize,
        ) -> Result<()> {
            for (alias, d) in deps {
                *count += 1;
                if *count > MAX_ATTEMPTS || path.len() > MAX_PACKAGES {
                    return Err(Error::new(
                        "RESOURCE_LIMIT",
                        "Dependency path budget exceeded",
                        2,
                    ));
                }
                let source = m.source(d)?;
                let id = source.id();
                let mut chain = path.to_vec();
                if chain.is_empty() {
                    chain.push(alias.clone());
                }
                if chain.contains(&id) {
                    chain.push(id.clone());
                    let mut e = Error::new("DEPENDENCY_CYCLE", chain.join(" -> "), 1);
                    e.chains = vec![chain];
                    return Err(e);
                }
                chain.push(id.clone());
                out.entry(id.clone()).or_default().push(Requirement {
                    source,
                    dependency: d.clone(),
                    chain: chain.clone(),
                });
                if out.len() > MAX_PACKAGES {
                    return Err(Error::new("RESOURCE_LIMIT", "Too many packages", 2));
                }
                if let Some(p) = selected.get(&id) {
                    visit(m, selected, &p.metadata.dependencies, &chain, out, count)?;
                }
            }
            Ok(())
        }
        let mut out = BTreeMap::new();
        visit(
            self.manifest,
            selected,
            &self.manifest.dependencies,
            &[],
            &mut out,
            &mut 0,
        )?;
        Ok(out)
    }
    fn finish(&self, selected: BTreeMap<String, LockedPackage>) -> Result<Lock> {
        let mut lock = Lock {
            schema_version: SCHEMA,
            resolver_version: RESOLVER.into(),
            manifest_digest: self.manifest.digest()?,
            roots: BTreeMap::new(),
            packages: selected,
        };
        for (alias, d) in &self.manifest.dependencies {
            lock.roots.insert(
                alias.clone(),
                Edge {
                    package: self.manifest.source(d)?.id(),
                    request: d.request(),
                },
            );
        }
        for p in lock.packages.values_mut() {
            for (alias, d) in &p.metadata.dependencies {
                p.dependencies.insert(
                    alias.clone(),
                    Edge {
                        package: self.manifest.source(d)?.id(),
                        request: d.request(),
                    },
                );
            }
        }
        lock.validate()?;
        Ok(lock)
    }
    fn search(&mut self, mut selected: BTreeMap<String, LockedPackage>) -> Result<Lock> {
        // Explicit heap frames keep the bounded graph from exhausting the native stack.
        let mut frames: Vec<Frame> = Vec::new();
        let mut last_error = None;
        loop {
            self.attempts += 1;
            if self.attempts > MAX_ATTEMPTS {
                return Err(Error::new(
                    "RESOURCE_LIMIT",
                    "Resolution backtracking budget exceeded",
                    2,
                ));
            }
            let expansion = self.requirements(&selected).and_then(|requirements| {
                for (id, p) in &selected {
                    if let Some(reqs) = requirements.get(id)
                        && !compatible(p, reqs)
                    {
                        return Err(conflict(id, reqs));
                    }
                }
                Ok(requirements
                    .into_iter()
                    .find(|(id, _)| !selected.contains_key(id)))
            });
            match expansion {
                Ok(Some((id, reqs))) => {
                    let old = self.previous.and_then(|l| l.packages.get(&id)).cloned();
                    let preferred = old
                        .as_ref()
                        .filter(|p| !self.update.contains(&id) && compatible(p, &reqs))
                        .map(|p| p.candidate.clone());
                    frames.push(Frame {
                        selected: selected.clone(),
                        id,
                        reqs,
                        old,
                        candidates: preferred.iter().cloned().collect(),
                        preferred,
                        catalog_pending: true,
                    });
                }
                Ok(None) => match self.finish(selected.clone()) {
                    Ok(lock) => return Ok(lock),
                    Err(e) if backtrack(&e) => last_error = Some(e),
                    Err(e) => return Err(e),
                },
                Err(e) if backtrack(&e) => last_error = Some(e),
                Err(e) => return Err(e),
            }
            loop {
                let Some(frame) = frames.last_mut() else {
                    return Err(last_error
                        .unwrap_or_else(|| Error::new("VERSION_CONFLICT", "No resolution", 1)));
                };
                let first = &frame.reqs[0];
                if frame.candidates.is_empty() && frame.catalog_pending {
                    frame.catalog_pending = false;
                    let mut available =
                        self.provider.candidates(&first.source, &first.dependency)?;
                    if available.len() > MAX_CANDIDATES {
                        return Err(Error::new("RESOURCE_LIMIT", "Candidate budget exceeded", 2));
                    }
                    available.sort_by(|a, b| {
                        let av = a
                            .version
                            .as_ref()
                            .and_then(|v| semver::Version::parse(v).ok());
                        let bv = b
                            .version
                            .as_ref()
                            .and_then(|v| semver::Version::parse(v).ok());
                        bv.cmp(&av).then_with(|| a.selector.cmp(&b.selector))
                    });
                    available.dedup();
                    frame.candidates = available
                        .into_iter()
                        .filter(|c| {
                            Some(c) != frame.preferred.as_ref()
                                && frame.reqs.iter().all(|r| c.matches(&r.dependency))
                        })
                        .collect();
                }
                if let Some(candidate) = frame.candidates.pop_front() {
                    let package = self.provider.materialize(
                        &first.source,
                        &candidate,
                        &first.dependency,
                        frame.old.as_ref(),
                    )?;
                    if !compatible(&package, &frame.reqs) {
                        continue;
                    }
                    selected = frame.selected.clone();
                    selected.insert(frame.id.clone(), package);
                    break;
                }
                if last_error.is_none() {
                    last_error = Some(conflict(&frame.id, &frame.reqs));
                }
                frames.pop();
            }
        }
    }
}
struct Frame {
    selected: BTreeMap<String, LockedPackage>,
    id: String,
    reqs: Vec<Requirement>,
    old: Option<LockedPackage>,
    preferred: Option<Candidate>,
    candidates: std::collections::VecDeque<Candidate>,
    catalog_pending: bool,
}

fn compatible(p: &LockedPackage, reqs: &[Requirement]) -> bool {
    reqs.iter().all(|r| {
        p.candidate.matches(&r.dependency)
            && match p.source {
                Source::Archive { .. } => {
                    r.dependency.sha256.as_deref().map(str::to_ascii_lowercase)
                        == p.acquisition.sha256.as_deref().map(str::to_ascii_lowercase)
                        && r.dependency.subdir == p.acquisition.subdir
                }
                Source::Git { .. } => {
                    r.dependency.tag_pattern.as_deref().unwrap_or("v{version}")
                        == p.acquisition.tag_pattern.as_deref().unwrap_or("v{version}")
                        || r.dependency.rev.is_some()
                }
                _ => true,
            }
    })
}
fn backtrack(e: &Error) -> bool {
    matches!(
        e.code.as_str(),
        "VERSION_CONFLICT" | "DEPENDENCY_CYCLE" | "DIRECTORY_CONFLICT"
    )
}
fn conflict(id: &str, reqs: &[Requirement]) -> Error {
    let mut e = Error::new(
        "VERSION_CONFLICT",
        format!(
            "No single candidate satisfies {}",
            reqs.iter()
                .map(|r| r.dependency.request())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        1,
    )
    .phase("resolution")
    .package(id)
    .hint("Align the constraints on the reported dependency paths, then run lock.");
    e.chains = reqs.iter().map(|r| r.chain.clone()).collect();
    e
}
