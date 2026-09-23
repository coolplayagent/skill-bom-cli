mod common;
use common::*;
use skill_bom::{domain::*, installer, store::Store};

#[test]
fn install_replace_remove_and_drift_protection() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = Store::new(tmp.path().join("cache"));
    let target = tmp.path().join("target");
    let first = lock(vec![package(&cache, "example", "1.0.0")]);
    assert_eq!(
        installer::plan(&target, "owner", &first).unwrap().changes[0].action,
        "add"
    );
    assert!(!target.exists());
    let guard = installer::acquire(&target, "owner").unwrap();
    installer::deploy(&target, "owner", &first, &cache, &guard).unwrap();
    let state = installer::read(&target, "owner").unwrap().unwrap();
    assert!(installer::verify(&target, &state, &first).unwrap().clean());
    assert!(
        installer::plan(&target, "owner", &first)
            .unwrap()
            .changes
            .is_empty()
    );
    assert!(installer::acquire(&target, "owner").is_err());
    assert!(installer::read(&target, "other").is_err());
    let next = lock(vec![package(&cache, "example", "1.1.0")]);
    assert_eq!(
        installer::plan(&target, "owner", &next).unwrap().changes[0].action,
        "replace"
    );
    std::fs::write(target.join("example/SKILL.md"), "local change").unwrap();
    let plan = installer::plan(&target, "owner", &next).unwrap();
    assert!(!plan.conflicts.is_empty());
    assert!(installer::deploy(&target, "owner", &next, &cache, &guard).is_err());
    assert_eq!(
        std::fs::read(target.join("example/SKILL.md")).unwrap(),
        b"local change"
    );
    std::fs::remove_dir_all(target.join("example")).unwrap();
    let p = first.packages.values().next().unwrap();
    skill_bom::store::copy_tree(&cache.get(p).unwrap(), &target.join("example"), &p.files).unwrap();
    installer::deploy(&target, "owner", &next, &cache, &guard).unwrap();
    let state = installer::read(&target, "owner").unwrap().unwrap();
    assert!(installer::verify(&target, &state, &next).unwrap().clean());
    let empty = lock(vec![]);
    assert_eq!(
        installer::deploy(&target, "owner", &empty, &cache, &guard)
            .unwrap()
            .changes[0]
            .action,
        "remove"
    );
    assert!(!target.join("example").exists());
}
#[test]
fn every_mutation_failure_rolls_back_prior_state() {
    for fail_at in ["before", "backed_up", "replaced", "recorded"] {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Store::new(tmp.path().join("cache"));
        let target = tmp.path().join("target");
        let first = lock(vec![
            package(&cache, "old", "1.0.0"),
            package(&cache, "replace", "1.0.0"),
        ]);
        let next = lock(vec![
            package(&cache, "new", "1.0.0"),
            package(&cache, "replace", "1.1.0"),
        ]);
        let guard = installer::acquire(&target, "owner").unwrap();
        installer::deploy(&target, "owner", &first, &cache, &guard).unwrap();
        let error =
            installer::deploy_with_hook(&target, "owner", &next, &cache, &guard, |phase, _| {
                if phase == fail_at {
                    Err(Error::new("INJECTED", "fault", 2))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(error.code, "INJECTED");
        let state = installer::read(&target, "owner").unwrap().unwrap();
        assert!(
            installer::verify(&target, &state, &first).unwrap().clean(),
            "{fail_at}"
        );
        assert!(!target.join("new").exists());
        assert!(!target.join(".skill-bom/transaction.json").exists());
    }
}
#[test]
fn panic_simulates_crash_and_next_writer_recovers() {
    for phase in ["before", "backed_up", "replaced", "recorded"] {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Store::new(tmp.path().join("cache"));
        let target = tmp.path().join("target");
        let first = lock(vec![package(&cache, "example", "1.0.0")]);
        let next = lock(vec![
            package(&cache, "example", "1.1.0"),
            package(&cache, "new", "1.0.0"),
        ]);
        let guard = installer::acquire(&target, "owner").unwrap();
        installer::deploy(&target, "owner", &first, &cache, &guard).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            installer::deploy_with_hook(&target, "owner", &next, &cache, &guard, |at, _| {
                assert_ne!(at, phase, "simulated crash");
                Ok(())
            })
        }));
        assert!(result.is_err());
        drop(guard);
        assert!(target.join(".skill-bom/transaction.json").exists());
        let guard = installer::acquire(&target, "owner").unwrap();
        let state = installer::read(&target, "owner").unwrap().unwrap();
        assert!(installer::verify(&target, &state, &first).unwrap().clean());
        installer::deploy(&target, "owner", &next, &cache, &guard).unwrap();
    }
}
#[test]
fn unmanaged_case_collisions_cache_failures_and_record_tampering() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = Store::new(tmp.path().join("cache"));
    let target = tmp.path().join("target");
    let locked = lock(vec![package(&cache, "example", "1.0.0")]);
    std::fs::create_dir_all(target.join("EXAMPLE")).unwrap();
    let plan = installer::plan(&target, "owner", &locked).unwrap();
    assert_eq!(plan.conflicts.len(), 1);
    assert!(target.join("EXAMPLE").exists());
    std::fs::remove_dir_all(target.join("EXAMPLE")).unwrap();
    let guard = installer::acquire(&target, "owner").unwrap();
    let p = locked.packages.values().next().unwrap();
    std::fs::write(
        cache.path(&p.tree_sha256).unwrap().join("SKILL.md"),
        "corrupt",
    )
    .unwrap();
    assert!(installer::deploy(&target, "owner", &locked, &cache, &guard).is_err());
    assert!(!target.join("example").exists());
    package(&cache, "example", "1.0.0");
    installer::deploy(&target, "owner", &locked, &cache, &guard).unwrap();
    let state = installer::read(&target, "owner").unwrap().unwrap();
    std::fs::create_dir(target.join("manual")).unwrap();
    assert_eq!(
        installer::verify(&target, &state, &locked)
            .unwrap()
            .unmanaged,
        vec!["manual"]
    );
    assert!(
        installer::verify(&target, &state, &lock(vec![]))
            .unwrap()
            .lock_differs
    );
    std::fs::remove_dir_all(target.join("example")).unwrap();
    assert_eq!(
        installer::verify(&target, &state, &locked)
            .unwrap()
            .packages
            .values()
            .next()
            .unwrap(),
        "missing"
    );
    let mut value = serde_json::to_value(state).unwrap();
    value["owner"] = "attacker".into();
    std::fs::write(
        target.join(".skill-bom/state.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    assert!(installer::read(&target, "owner").is_err());
}
#[test]
fn control_and_symlink_ownership_are_not_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir_all(target.join(".skill-bom")).unwrap();
    assert!(installer::acquire(&target, "owner").is_err());
    #[cfg(unix)]
    {
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(installer::acquire(&link, "owner").is_err());
    }
}

#[test]
#[cfg(unix)]
fn executable_modes_are_install_facts_not_cache_keys() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Store::new(temp.path().join("cache"));
    let mut p = package(&cache, "example", "1.0.0");
    let original = cache.get(&p).unwrap();
    let bytes = std::fs::read(original.join("SKILL.md")).unwrap();
    p.files
        .iter_mut()
        .find(|f| f.path == "SKILL.md")
        .unwrap()
        .executable = true;
    assert!(cache.get(&p).is_ok());
    let target = temp.path().join("target");
    let lock = lock(vec![p]);
    let guard = installer::acquire(&target, "owner").unwrap();
    installer::deploy(&target, "owner", &lock, &cache, &guard).unwrap();
    let state = installer::read(&target, "owner").unwrap().unwrap();
    assert!(installer::verify(&target, &state, &lock).unwrap().clean());
    assert_eq!(std::fs::read(original.join("SKILL.md")).unwrap(), bytes);
    skill_bom::store::set_executable(&target.join("example/SKILL.md"), false).unwrap();
    assert!(!installer::verify(&target, &state, &lock).unwrap().clean());
}
