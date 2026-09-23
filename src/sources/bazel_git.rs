//! Isolated Bazel crate for the Git source adapter.
pub use skill_bom_foundation::{domain, paths, process, store};

#[path = "git.rs"]
pub mod git;
