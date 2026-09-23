//! Per-target ownership, preflight, journaled directory replacement and recovery.
use crate::domain::*;
use crate::{paths, store};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Installed {
    pub schema_version: u32,
    pub owner: String,
    pub lock_digest: String,
    pub lock: Lock,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub directory: String,
    pub action: String,
    pub package: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub conflicts: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    pub packages: BTreeMap<String, String>,
    pub unmanaged: Vec<String>,
    pub lock_differs: bool,
    pub issues: Vec<String>,
}
impl Verification {
    pub fn clean(&self) -> bool {
        !self.lock_differs
            && self.issues.is_empty()
            && self.packages.values().all(|v| v == "verified")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    committed: bool,
    old: Option<Installed>,
    new: Installed,
    changes: Vec<Change>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema_version: u32,
    owner: String,
}
fn control(target: &Path) -> PathBuf {
    target.join(".skill-bom")
}
fn ownership(target: &Path, owner: &str) -> Result<()> {
    paths::no_symlink(target)?;
    let path = control(target).join("owner.json");
    if path.exists() {
        let record: Owner = serde_json::from_slice(&paths::read(&path, 4096)?)?;
        if record.schema_version != SCHEMA || record.owner != owner {
            return fail(
                "TARGET_OWNERSHIP",
                "Target belongs to a different installation environment",
            );
        }
    } else if control(target).exists() {
        return fail(
            "UNMANAGED_CONTROL",
            "Existing .skill-bom directory has no ownership record",
        );
    }
    Ok(())
}
pub fn read(target: &Path, owner: &str) -> Result<Option<Installed>> {
    ownership(target, owner)?;
    let path = control(target).join("state.json");
    if !path.exists() {
        return Ok(None);
    }
    let state: Installed = serde_json::from_slice(&paths::read(&path, 32 * 1024 * 1024)?)?;
    if state.schema_version != SCHEMA
        || state.owner != owner
        || state.lock_digest != json_digest(&state.lock)?
    {
        return fail("INSTALL_RECORD", "Invalid installation record");
    }
    state.lock.validate()?;
    Ok(Some(state))
}
pub fn verify(target: &Path, state: &Installed, current: &Lock) -> Result<Verification> {
    ownership(target, &state.owner)?;
    let mut result = Verification {
        packages: BTreeMap::new(),
        unmanaged: vec![],
        lock_differs: state.lock_digest != json_digest(current)?,
        issues: vec![],
    };
    let names: BTreeSet<_> = state
        .lock
        .packages
        .values()
        .map(|p| p.directory.as_str())
        .collect();
    for (id, p) in &state.lock.packages {
        let path = target.join(&p.directory);
        let status = match store::inventory(&path) {
            Ok(files)
                if store::tree_digest(&files).is_ok_and(|hash| hash == p.tree_sha256)
                    && store::same_files(&files, &p.files) =>
            {
                "verified"
            }
            Ok(_) => "modified",
            Err(_) if !path.exists() => "missing",
            Err(_) => "unreadable_or_unsafe",
        };
        result.packages.insert(id.clone(), status.into());
    }
    if target.exists() {
        for entry in std::fs::read_dir(target)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name != ".skill-bom" && !names.contains(name.as_str()) {
                result.unmanaged.push(name);
            }
        }
    }
    result.unmanaged.sort();
    if control(target).join("transaction.json").exists() {
        result.issues.push(
            "Unfinished transaction; run install to recover before reading installed content"
                .into(),
        );
    }
    Ok(result)
}
pub fn plan(target: &Path, owner: &str, lock: &Lock) -> Result<Plan> {
    lock.validate()?;
    let old = read(target, owner)?;
    let mut plan = Plan {
        changes: vec![],
        conflicts: vec![],
    };
    let desired: BTreeMap<_, _> = lock
        .packages
        .iter()
        .map(|(id, p)| (p.directory.as_str(), (id, p)))
        .collect();
    let previous: BTreeMap<_, _> = old
        .as_ref()
        .map(|s| {
            s.lock
                .packages
                .iter()
                .map(|(id, p)| (p.directory.as_str(), (id, p)))
                .collect()
        })
        .unwrap_or_default();
    let mut existing = BTreeMap::new();
    if target.exists() {
        for e in std::fs::read_dir(target)? {
            let name = e?.file_name().to_string_lossy().into_owned();
            existing.insert(name.to_lowercase(), name);
        }
    }
    for (name, (id, p)) in &desired {
        if let Some((_, before)) = previous.get(name) {
            if p.tree_sha256 != before.tree_sha256 || !store::same_files(&p.files, &before.files) {
                plan.changes.push(Change {
                    directory: (*name).into(),
                    action: "replace".into(),
                    package: (*id).clone(),
                });
            }
        } else if let Some(found) = existing.get(&name.to_lowercase()) {
            plan.conflicts
                .push(format!("Unmanaged directory collision: {found}"));
        } else {
            plan.changes.push(Change {
                directory: (*name).into(),
                action: "add".into(),
                package: (*id).clone(),
            });
        }
    }
    for (name, (id, _)) in &previous {
        if !desired.contains_key(name) {
            plan.changes.push(Change {
                directory: (*name).into(),
                action: "remove".into(),
                package: (*id).clone(),
            });
        }
    }
    if let Some(old) = &old {
        let status = verify(target, old, &old.lock)?;
        for (id, s) in status.packages {
            if s != "verified" {
                plan.conflicts.push(format!(
                    "Managed package {id} is {s}; preserve or restore local changes first"
                ));
            }
        }
        plan.conflicts.extend(status.issues);
    }
    plan.changes.sort_by(|a, b| a.directory.cmp(&b.directory));
    Ok(plan)
}
pub struct TargetGuard {
    _file: std::fs::File,
}
pub fn acquire(target: &Path, owner: &str) -> Result<TargetGuard> {
    paths::no_symlink(target)?;
    let dir = control(target);
    let existed = dir.exists();
    if existed {
        ownership(target, owner)?;
    }
    std::fs::create_dir_all(&dir)?;
    paths::no_symlink(&dir.join("write.lock"))?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("write.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|_| Error::new("TARGET_BUSY", "Another installation is using the target", 2))?;
    if dir.join("owner.json").exists() {
        ownership(target, owner)?;
    } else {
        let entries = std::fs::read_dir(&dir)?.collect::<std::io::Result<Vec<_>>>()?;
        if entries.iter().any(|e| e.file_name() != "write.lock") {
            return fail(
                "UNMANAGED_CONTROL",
                "Refusing to adopt an unrecognized control directory",
            );
        }
        paths::atomic_write(
            &dir.join("owner.json"),
            &serde_json::to_vec(&Owner {
                schema_version: SCHEMA,
                owner: owner.into(),
            })?,
        )?;
    }
    let guard = TargetGuard { _file: file };
    recover(target, owner, &guard)?;
    Ok(guard)
}
pub fn deploy(
    target: &Path,
    owner: &str,
    lock: &Lock,
    cache: &store::Store,
    guard: &TargetGuard,
) -> Result<Plan> {
    deploy_with_hook(target, owner, lock, cache, guard, |_, _| Ok(()))
}
/// Fault injection hook is part of the deterministic transaction test boundary.
pub fn deploy_with_hook(
    target: &Path,
    owner: &str,
    lock: &Lock,
    cache: &store::Store,
    _guard: &TargetGuard,
    mut hook: impl FnMut(&str, usize) -> Result<()>,
) -> Result<Plan> {
    let plan = plan(target, owner, lock)?;
    if !plan.conflicts.is_empty() {
        return fail("INSTALL_CONFLICT", plan.conflicts.join("; "));
    }
    let dir = control(target);
    let txn = dir.join("transaction");
    paths::no_symlink(&txn)?;
    if txn.exists() {
        std::fs::remove_dir_all(&txn)?;
    }
    std::fs::create_dir_all(txn.join("staged"))?;
    std::fs::create_dir_all(txn.join("backup"))?;
    // All bytes are verified and staged before publishing the journal or replacing a package.
    for change in &plan.changes {
        if change.action != "remove" {
            let p = &lock.packages[&change.package];
            let source = cache.get(p)?;
            store::copy_tree(
                &source,
                &txn.join("staged").join(&change.directory),
                &p.files,
            )?;
        }
    }
    let new = Installed {
        schema_version: SCHEMA,
        owner: owner.into(),
        lock_digest: json_digest(lock)?,
        lock: lock.clone(),
    };
    let mut journal = Journal {
        schema_version: SCHEMA,
        committed: false,
        old: read(target, owner)?,
        new,
        changes: plan.changes.clone(),
    };
    let journal_path = dir.join("transaction.json");
    paths::atomic_write(&journal_path, &serde_json::to_vec_pretty(&journal)?)?;
    let operation: Result<()> = (|| {
        for (index, change) in journal.changes.iter().enumerate() {
            crate::process::check_interrupt()?;
            hook("before", index)?;
            let path = target.join(&change.directory);
            if change.action != "add" {
                std::fs::rename(&path, txn.join("backup").join(&change.directory))?;
                paths::sync_dir(target)?;
            }
            hook("backed_up", index)?;
            if change.action != "remove" {
                std::fs::rename(txn.join("staged").join(&change.directory), &path)?;
                paths::sync_dir(target)?;
            }
            hook("replaced", index)?;
        }
        paths::atomic_write(
            &dir.join("state.json"),
            &serde_json::to_vec_pretty(&journal.new)?,
        )?;
        hook("recorded", journal.changes.len())?;
        journal.committed = true;
        paths::atomic_write(&journal_path, &serde_json::to_vec_pretty(&journal)?)?;
        Ok(())
    })();
    if let Err(error) = operation {
        if let Err(recovery) = rollback(target, &journal) {
            return Err(Error::new(
                "RECOVERY_REQUIRED",
                format!("{}; rollback failed: {}", error.code, recovery.code),
                2,
            )
            .hint("Keep .skill-bom intact and run install again to recover."));
        }
        cleanup(target)?;
        return Err(error);
    }
    cleanup(target)?;
    Ok(plan)
}
fn validate_journal(j: &Journal, owner: &str) -> Result<()> {
    if j.schema_version != SCHEMA
        || j.new.owner != owner
        || j.new.lock_digest != json_digest(&j.new.lock)?
    {
        return fail(
            "TRANSACTION_INVALID",
            "Invalid transaction ownership or digest",
        );
    }
    j.new.lock.validate()?;
    if let Some(old) = &j.old {
        if old.owner != owner || old.lock_digest != json_digest(&old.lock)? {
            return fail("TRANSACTION_INVALID", "Invalid previous record");
        }
        old.lock.validate()?;
    }
    let mut names = BTreeSet::new();
    for c in &j.changes {
        if !safe_name(&c.directory)
            || !names.insert(c.directory.to_lowercase())
            || !["add", "replace", "remove"].contains(&c.action.as_str())
        {
            return fail("TRANSACTION_INVALID", "Invalid transaction operation");
        }
        let old_has = j
            .old
            .as_ref()
            .is_some_and(|s| s.lock.packages.values().any(|p| p.directory == c.directory));
        let new_has = j
            .new
            .lock
            .packages
            .values()
            .any(|p| p.directory == c.directory);
        if old_has != (c.action != "add") || new_has != (c.action != "remove") {
            return fail(
                "TRANSACTION_INVALID",
                "Transaction operation differs from inventory",
            );
        }
    }
    Ok(())
}
pub fn recover(target: &Path, owner: &str, _guard: &TargetGuard) -> Result<()> {
    let path = control(target).join("transaction.json");
    if !path.exists() {
        return Ok(());
    }
    let j: Journal = serde_json::from_slice(&paths::read(&path, 64 * 1024 * 1024)?)?;
    validate_journal(&j, owner)?;
    if !j.committed {
        rollback(target, &j)?;
    }
    cleanup(target)
}
fn rollback(target: &Path, j: &Journal) -> Result<()> {
    let dir = control(target);
    let txn = dir.join("transaction");
    for c in j.changes.iter().rev() {
        let target_path = target.join(&c.directory);
        let backup = txn.join("backup").join(&c.directory);
        let staged = txn.join("staged").join(&c.directory);
        paths::no_symlink(&target_path)?;
        paths::no_symlink(&backup)?;
        paths::no_symlink(&staged)?;
        if backup.exists() {
            if target_path.exists() {
                std::fs::remove_dir_all(&target_path)?;
            }
            std::fs::rename(backup, target_path)?;
        } else if c.action == "add" && !staged.exists() && target_path.exists() {
            std::fs::remove_dir_all(target_path)?;
        }
    }
    if let Some(old) = &j.old {
        paths::atomic_write(&dir.join("state.json"), &serde_json::to_vec_pretty(old)?)?;
    } else if dir.join("state.json").exists() {
        std::fs::remove_file(dir.join("state.json"))?;
    }
    paths::sync_dir(target)
}
fn cleanup(target: &Path) -> Result<()> {
    let dir = control(target);
    let txn = dir.join("transaction");
    paths::no_symlink(&txn)?;
    // The committed/rolled-back state is durable before removal of the journal.
    std::fs::remove_file(dir.join("transaction.json"))?;
    paths::sync_dir(&dir)?;
    if txn.exists() {
        std::fs::remove_dir_all(txn)?;
    }
    Ok(())
}
