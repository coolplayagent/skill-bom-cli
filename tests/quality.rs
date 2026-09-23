mod common;
use common::*;
use skill_bom::{bom, config::Manifest, domain::*, store::Store};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
fn root() -> PathBuf {
    repository_root()
}
fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "target"
            || name == "verification"
            || name == ".git"
            || name == ".qualitygate"
            || name.starts_with("bazel-")
        {
            continue;
        }
        if e.file_type().unwrap().is_dir() {
            files(&e.path(), out);
        } else if e.path().is_file() {
            out.push(e.path());
        }
    }
}
#[test]
fn authored_files_are_bounded_and_rust_is_safe() {
    let mut all = vec![];
    files(&root(), &mut all);
    for file in all {
        let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !["rs", "md", "toml", "yml", "yaml", "bazel", "bzl"].contains(&ext) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.lines().count() <= 1000,
            "{} exceeds 1000 lines",
            file.display()
        );
        if ext == "rs" {
            let syntax = syn::parse_file(&text).unwrap();
            let mut visitor = UnsafeCheck::default();
            visitor.visit_file(&syntax);
            assert!(!visitor.found, "unsafe code in {}", file.display());
        }
    }
}
#[derive(Default)]
struct UnsafeCheck {
    found: bool,
}
impl<'ast> Visit<'ast> for UnsafeCheck {
    fn visit_expr_unsafe(&mut self, _: &'ast syn::ExprUnsafe) {
        self.found = true;
    }
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.found |= node.sig.unsafety.is_some();
        syn::visit::visit_item_fn(self, node);
    }
}
#[derive(Default)]
struct Imports {
    dependencies: BTreeSet<String>,
    paths: Vec<String>,
}
impl<'ast> Visit<'ast> for Imports {
    fn visit_path(&mut self, p: &'ast syn::Path) {
        let parts: Vec<_> = p.segments.iter().map(|p| p.ident.to_string()).collect();
        if parts.first().is_some_and(|p| p == "crate") && parts.len() > 1 {
            self.dependencies.insert(parts[1].clone());
        }
        self.paths.push(parts.join("::"));
        syn::visit::visit_path(self, p);
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        fn walk(tree: &syn::UseTree, prefix: Vec<String>, out: &mut Imports) {
            match tree {
                syn::UseTree::Path(p) => {
                    let mut prefix = prefix;
                    prefix.push(p.ident.to_string());
                    walk(&p.tree, prefix, out);
                }
                syn::UseTree::Group(g) => {
                    for t in &g.items {
                        walk(t, prefix.clone(), out);
                    }
                }
                syn::UseTree::Name(n) => {
                    let mut path = prefix;
                    path.push(n.ident.to_string());
                    if path.first().is_some_and(|s| s == "crate") && path.len() > 1 {
                        out.dependencies.insert(path[1].clone());
                    }
                    out.paths.push(path.join("::"));
                }
                _ => (),
            }
        }
        walk(&item.tree, vec![], self);
    }
}
#[test]
fn module_graph_is_acyclic_and_io_has_owners() {
    let mut source_files = vec![];
    files(&root().join("src"), &mut source_files);
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for file in source_files {
        let rel = file.strip_prefix(root().join("src")).unwrap();
        let first = rel
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_string_lossy();
        let owner = first.trim_end_matches(".rs");
        if owner == "lib" || owner == "main" || owner.ends_with("_tests") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let mut imports = Imports::default();
        imports.visit_file(&syn::parse_file(&text).unwrap());
        for dep in &imports.dependencies {
            if dep != owner {
                graph.entry(owner.into()).or_default().insert(dep.clone());
            }
        }
        for path in imports.paths {
            if owner == "domain" {
                assert!(
                    ![
                        "std::fs",
                        "std::process",
                        "reqwest",
                        "crate::env",
                        "crate::paths",
                        "crate::net",
                        "crate::process"
                    ]
                    .iter()
                    .any(|p| path.starts_with(p)),
                    "domain I/O: {path}"
                );
            }
            if path.starts_with("reqwest") {
                assert_eq!(owner, "net");
            }
            if path.starts_with("std::env") {
                assert_eq!(owner, "env");
            }
            if path.starts_with("std::process::Command") {
                assert_eq!(owner, "process");
            }
        }
        if matches!(
            owner,
            "domain" | "resolver" | "sources" | "store" | "installer" | "bom"
        ) {
            assert!(!imports.dependencies.contains("interfaces"));
        }
    }
    fn visit<'a>(
        node: &'a str,
        graph: &'a BTreeMap<String, BTreeSet<String>>,
        active: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) {
        if done.contains(node) {
            return;
        }
        assert!(active.insert(node), "architecture cycle at {node}");
        if let Some(edges) = graph.get(node) {
            for edge in edges {
                visit(edge, graph, active, done);
            }
        }
        active.remove(node);
        done.insert(node);
    }
    for node in graph.keys() {
        visit(node, &graph, &mut BTreeSet::new(), &mut BTreeSet::new());
    }
}
#[test]
fn docs_examples_and_build_contracts_agree() {
    Manifest::parse(include_str!("../examples/skills.toml")).unwrap();
    let _: PackageManifest = toml::from_str(include_str!("../examples/skill.toml")).unwrap();
    let requirements = include_str!("../codespec/requirements/skill-bom-cli.md");
    let tests = include_str!("../codespec/test/skill-bom-cli.md");
    for id in 1..=13 {
        assert!(requirements.contains(&format!("R{id:02}")));
        assert!(tests.contains(&format!("R{id:02}")));
    }
    let cargo: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    assert_eq!(cargo["package"]["edition"].as_str(), Some("2024"));
    let module = include_str!("../MODULE.bazel");
    let toolchain = include_str!("../rust-toolchain.toml");
    assert!(module.contains("1.97.1") && toolchain.contains("1.97.1"));
    assert_eq!(include_str!("../.bazelversion").trim(), "9.2.0");
    let build = include_str!("../BUILD.bazel");
    for test in [
        "config",
        "resolver",
        "sources",
        "protocol",
        "transactions",
        "cli",
        "quality",
    ] {
        assert!(build.contains(&format!("\"{test}\"")));
    }
    assert!(
        root().join("MODULE.bazel.lock").is_file(),
        "Bazel module lock must be generated and checked in"
    );
}
#[test]
fn bom_checksums_unknowns_and_reference_integrity() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Store::new(temp.path().join("cache"));
    let mut p = package(&cache, "example", "1.0.0");
    p.metadata.license = None;
    p.metadata.dependency_metadata = MetadataKind::Unknown;
    p.source = Source::Clawhub {
        registry: "https://clawhub.ai".into(),
        owner: "owner".into(),
        slug: "example".into(),
    };
    let lock = lock(vec![p]);
    let bom = bom::native("project", &lock, "2026-01-01T00:00:00Z".into(), None).unwrap();
    let mut spdx = bom::spdx(&bom).unwrap();
    assert_eq!(spdx["packages"][1]["licenseDeclared"], "NOASSERTION");
    assert!(spdx["packages"][1].get("checksums").is_none());
    assert_eq!(spdx["packages"][1]["filesAnalyzed"], false);
    assert!(!serde_json::to_string(&spdx).unwrap().contains("pkg:skill"));
    let text = serde_json::to_string(&bom).unwrap();
    assert!(!text.contains(temp.path().to_str().unwrap()));
    assert!(text.contains("unknown"));
    spdx["relationships"][0]["relatedSpdxElement"] = "missing".into();
    assert!(bom::validate_references(&spdx).is_err());
    let mut spdx = bom::spdx(&bom).unwrap();
    spdx["packages"][1]["SPDXID"] = "SPDXRef-Root".into();
    assert!(bom::validate_references(&spdx).is_err());
    assert!(bom::validate_references(&serde_json::json!({})).is_err());
}
