//! Command orchestration. Core/source/store contracts remain independent of CLI rendering.
use crate::domain::*;
use crate::interfaces::{BomFrom, Cli, Command, Format, Output, SchemaKind};
use crate::{bom, config, env, installer, paths, resolver, sources, store};
use std::collections::BTreeSet;
use std::io::Write;

pub fn run(cli: &Cli) -> Result<Output> {
    if let Command::Schema { kind } = &cli.command {
        return schema(*kind);
    }
    if cli.format == Format::SpdxJson && !matches!(cli.command, Command::Bom { .. }) {
        return Err(Error::new(
            "FORMAT",
            "spdx-json is only available for bom",
            2,
        ));
    }
    let mut scope = config::Scope::new(cli.manifest.as_deref(), cli.global, cli.target.as_deref())?;
    if matches!(cli.command, Command::Init) {
        paths::no_symlink(&scope.manifest)?;
        std::fs::create_dir_all(scope.manifest.parent().unwrap())?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&scope.manifest)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::new(
                        "MANIFEST_EXISTS",
                        "Manifest already exists; init never overwrites it",
                        1,
                    )
                } else {
                    e.into()
                }
            })?;
        file.write_all(
            b"schema_version = 1\n\n[project]\nname = \"my-project\"\n\n[dependencies]\n",
        )?;
        file.sync_all()?;
        return Output::json(serde_json::json!({"created":scope.manifest}));
    }
    let manifest = scope.load(cli.target.is_some())?;
    eprintln!(
        "manifest: {}\nlock: {}\ntarget: {}",
        scope.manifest.display(),
        scope.lock.display(),
        scope.target.display()
    );
    match &cli.command {
        Command::Validate => {
            Output::json(serde_json::json!({"valid":true,"manifest_digest":manifest.digest()?}))
        }
        Command::Lock | Command::Update { .. } => {
            let old = optional_lock(&scope)?;
            let mut update = BTreeSet::new();
            if let Command::Update { alias } = &cli.command {
                if let Some(alias) = alias {
                    let d = manifest
                        .dependencies
                        .get(alias)
                        .ok_or_else(|| Error::new("ALIAS", "Unknown root dependency alias", 2))?;
                    update.insert(manifest.source(d)?.id());
                } else {
                    if let Some(old) = &old {
                        update.extend(old.packages.keys().cloned());
                    }
                    for d in manifest.dependencies.values() {
                        update.insert(manifest.source(d)?.id());
                    }
                }
            }
            let mut provider = sources::Provider::new(
                &manifest,
                scope.cache.clone(),
                cli.offline,
                cli.strict_metadata,
            )?;
            let lock = resolver::Resolver::new(&manifest, &mut provider, old.as_ref(), update)
                .resolve()?;
            config::write_lock(&scope.lock, &lock)?;
            report_warnings(&lock, cli.offline);
            Output::json(
                serde_json::json!({"lock":scope.lock,"packages":lock.packages.len(),"digest":json_digest(&lock)?}),
            )
        }
        Command::Install {
            locked,
            frozen,
            dry_run,
        } => {
            let existing = optional_lock(&scope)?;
            if (*locked || *frozen) && existing.is_none() {
                return fail("LOCK_REQUIRED", "--locked/--frozen requires skills.lock");
            }
            let offline = cli.offline || *frozen;
            let mut provider = sources::Provider::new(
                &manifest,
                scope.cache.clone(),
                offline,
                cli.strict_metadata,
            )?;
            let new = existing.is_none();
            let lock = match existing {
                Some(l) => {
                    if l.manifest_digest != manifest.digest()? {
                        return Err(Error::new(
                            "MANIFEST_CHANGED",
                            "Manifest differs from skills.lock",
                            1,
                        )
                        .hint("Run skill-bom lock to resolve the edited declaration."));
                    }
                    l
                }
                None => resolver::Resolver::new(&manifest, &mut provider, None, BTreeSet::new())
                    .resolve()?,
            };
            if cli.strict_metadata {
                strict(&lock)?;
            }
            for p in lock.packages.values() {
                provider.ensure(p)?;
            }
            report_warnings(&lock, offline);
            if *dry_run {
                let plan = installer::plan(&scope.target, &scope.owner, &lock)?;
                let failed = !plan.conflicts.is_empty();
                let mut out = Output::json(plan)?;
                out.exit_code = u8::from(failed);
                return Ok(out);
            }
            if new {
                config::write_lock(&scope.lock, &lock)?;
            }
            let guard = installer::acquire(&scope.target, &scope.owner)?;
            let plan =
                installer::deploy(&scope.target, &scope.owner, &lock, &provider.store, &guard)?;
            Output::json(plan)
        }
        Command::List => {
            let state = installer::read(&scope.target, &scope.owner)?;
            Output::json(serde_json::json!({"installed":state,"target":scope.target}))
        }
        Command::Verify => {
            let lock = config::read_lock(&scope.lock)?;
            let state = installed(&scope)?;
            let status = installer::verify(&scope.target, &state, &lock)?;
            let code = u8::from(!status.clean());
            let mut out = Output::json(status)?;
            out.exit_code = code;
            Ok(out)
        }
        Command::Tree => {
            let lock = config::read_lock(&scope.lock)?;
            let mut lines = vec![];
            let mut count = 0;
            for (alias, edge) in &lock.roots {
                lines.push(alias.clone());
                tree(&lock, &edge.package, 1, &mut lines, &mut count)?;
            }
            let mut out = Output::json(&lock)?;
            out.text = Some(lines.join("\n"));
            Ok(out)
        }
        Command::Why { package } => {
            let lock = config::read_lock(&scope.lock)?;
            let matches: Vec<_> = lock
                .packages
                .iter()
                .filter(|(id, p)| {
                    *id == package
                        || p.metadata.name == *package
                        || lock.roots.get(package).is_some_and(|e| e.package == **id)
                })
                .map(|(id, _)| id)
                .collect();
            if matches.len() != 1 {
                return fail(
                    "PACKAGE_QUERY",
                    "Package query is missing or ambiguous; use the full PackageId",
                );
            }
            let chains = lock.paths_to(matches[0])?;
            let mut out = Output::json(&chains)?;
            out.text = Some(
                chains
                    .iter()
                    .map(|p| p.join(" -> "))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            Ok(out)
        }
        Command::Bom { from, timestamp } => {
            let current = config::read_lock(&scope.lock)?;
            let (lock, verification) = match from {
                BomFrom::Lock => (current, None),
                BomFrom::Installed => {
                    let state = installed(&scope)?;
                    let status = installer::verify(&scope.target, &state, &current)?;
                    (state.lock, Some(status))
                }
            };
            if cli.strict_metadata {
                strict(&lock)?;
            }
            let code = u8::from(verification.as_ref().is_some_and(|v| !v.clean()));
            let bom = bom::native(
                &manifest.project.name,
                &lock,
                env::timestamp(timestamp.as_deref())?,
                verification,
            )?;
            let mut out = if cli.format == Format::SpdxJson {
                Output::json(bom::spdx(&bom)?)?
            } else {
                Output::json(bom)?
            };
            out.exit_code = code;
            Ok(out)
        }
        Command::Init | Command::Schema { .. } => unreachable!("handled before manifest loading"),
    }
}
fn optional_lock(scope: &config::Scope) -> Result<Option<Lock>> {
    if scope.lock.exists() {
        Ok(Some(config::read_lock(&scope.lock)?))
    } else {
        Ok(None)
    }
}
fn installed(scope: &config::Scope) -> Result<installer::Installed> {
    installer::read(&scope.target, &scope.owner)?.ok_or_else(|| {
        Error::new(
            "NOT_INSTALLED",
            "No installation record in this environment",
            1,
        )
    })
}
fn strict(lock: &Lock) -> Result<()> {
    for (id, p) in &lock.packages {
        if !p.metadata.complete() {
            return Err(Error::new(
                "METADATA_UNKNOWN",
                "Incomplete Skill dependency metadata",
                1,
            )
            .package(id));
        }
    }
    Ok(())
}
fn report_warnings(lock: &Lock, offline: bool) {
    for (id, p) in &lock.packages {
        for message in &p.metadata.diagnostics {
            eprintln!("warning: {id}: {message}");
        }
        if p.evidence
            .scan
            .as_ref()
            .is_some_and(|s| s.status == "suspicious")
        {
            eprintln!(
                "warning: {id}: ClawHub scan was suspicious at the recorded observation time"
            );
        }
    }
    if offline {
        eprintln!("warning: offline; remote security status has not been refreshed");
    }
}
fn tree(
    lock: &Lock,
    id: &str,
    depth: usize,
    out: &mut Vec<String>,
    count: &mut usize,
) -> Result<()> {
    *count += 1;
    if *count > MAX_ATTEMPTS {
        return Err(Error::new(
            "RESOURCE_LIMIT",
            "Tree output exceeds path budget",
            2,
        ));
    }
    let p = &lock.packages[id];
    out.push(format!(
        "{}{} {} [{}]",
        "  ".repeat(depth),
        p.metadata.name,
        p.candidate.display(),
        id
    ));
    for e in p.dependencies.values() {
        tree(lock, &e.package, depth + 1, out, count)?;
    }
    Ok(())
}
fn schema(kind: SchemaKind) -> Result<Output> {
    use schemars::schema_for;
    let schema = match kind {
        SchemaKind::Manifest => schema_for!(config::Manifest),
        SchemaKind::Package => schema_for!(PackageManifest),
        SchemaKind::Lock => schema_for!(Lock),
        SchemaKind::Bom => schema_for!(bom::Bom),
        SchemaKind::Installed => schema_for!(installer::Installed),
        SchemaKind::Error => schema_for!(Error),
    };
    Output::json(schema)
}
/// Expose the shared cache boundary for embedding applications.
pub fn cache(scope: &config::Scope) -> store::Store {
    store::Store::new(scope.cache.clone())
}
