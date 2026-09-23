//! Isolated Bazel crate for the HTTPS archive source adapter.
pub use skill_bom_foundation::{domain, net, paths, store};

#[path = "archive.rs"]
pub mod archive;
