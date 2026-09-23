#![allow(dead_code)]
pub mod http;
use skill_bom::{config::Manifest, domain::*, store::Store};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::Path;
pub fn manifest() -> Manifest {
    Manifest::parse("schema_version=1\n[project]\nname='test'\n").unwrap()
}
pub fn dep(name: &str, version: &str) -> Dependency {
    Dependency {
        git: Some(format!("https://example.org/{name}")),
        version: Some(version.into()),
        ..Dependency::default()
    }
}
pub fn metadata(name: &str, version: &str, deps: &str) -> String {
    format!(
        "schema_version=1\n[package]\nname={name:?}\nversion={version:?}\nlicense='MIT'\n{deps}"
    )
}
pub fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().unix_permissions(0o644),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
pub fn archive(name: &str, version: &str) -> Vec<u8> {
    zip(&[
        (
            "SKILL.md",
            format!("---\nname: {name}\n---\nInstruction text; never executed.\n").as_bytes(),
        ),
        ("skill.toml", metadata(name, version, "").as_bytes()),
    ])
}
pub fn tar(files: &[(&str, &[u8])], gzip: bool) -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append_data(&mut header, *name, *bytes).unwrap();
    }
    let bytes = archive.into_inner().unwrap();
    if !gzip {
        return bytes;
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&bytes).unwrap();
    gz.finish().unwrap()
}
pub fn package(cache: &Store, name: &str, version: &str) -> LockedPackage {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("SKILL.md"),
        format!("---\nname: {name}\n---\n{version}\n"),
    )
    .unwrap();
    std::fs::write(tmp.path().join("skill.toml"), metadata(name, version, "")).unwrap();
    let d = dep(name, &format!("={version}"));
    let source = manifest().source(&d).unwrap();
    let (hash, files) = cache.publish(tmp.path()).unwrap();
    LockedPackage {
        source,
        candidate: Candidate {
            version: Some(version.into()),
            revision: Some("a".repeat(40)),
            selector: format!("v{version}"),
        },
        metadata: Metadata {
            name: name.into(),
            license: Some("MIT".into()),
            description: None,
            dependency_metadata: MetadataKind::Upstream,
            metadata_digest: None,
            dependencies: BTreeMap::new(),
            diagnostics: vec![],
        },
        tree_sha256: hash,
        tree_algorithm: "skill-tree-sha256-v1".into(),
        files,
        evidence: Evidence::default(),
        dependencies: BTreeMap::new(),
        directory: name.into(),
        acquisition: d,
    }
}
pub fn lock(packages: Vec<LockedPackage>) -> Lock {
    let mut lock = Lock {
        schema_version: SCHEMA,
        resolver_version: RESOLVER.into(),
        manifest_digest: "a".repeat(64),
        roots: BTreeMap::new(),
        packages: BTreeMap::new(),
    };
    for p in packages {
        let id = p.source.id();
        lock.roots.insert(
            p.directory.clone(),
            Edge {
                package: id.clone(),
                request: p.acquisition.request(),
            },
        );
        lock.packages.insert(id, p);
    }
    lock
}
pub fn write_manifest(dir: &Path, url: &str, hash: &str) -> std::path::PathBuf {
    let path = dir.join("skills.toml");
    std::fs::write(&path,format!("schema_version=1\n[project]\nname='test'\n[dependencies.example]\nurl={url:?}\nversion='=1.0.0'\nsha256={hash:?}\n")).unwrap();
    path
}

pub fn repository_root() -> std::path::PathBuf {
    std::env::var_os("SKILL_BOM_TEST_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}
