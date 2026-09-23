//! Isolated Bazel crate for the ClawHub source adapter.
pub use skill_bom_foundation::{config, domain, env, net, paths, store};

#[path = "clawhub.rs"]
pub mod clawhub;
