//! Bazel source facade linking separately cached adapters.
pub use skill_bom_foundation::{config, domain, net, paths, store};
pub use skill_bom_source_agentcenter::agentcenter;
pub use skill_bom_source_archive::archive;
pub use skill_bom_source_clawhub::clawhub;
pub use skill_bom_source_git::git;

#[path = "mod.rs"]
pub mod sources;
