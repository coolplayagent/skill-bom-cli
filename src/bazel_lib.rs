//! Bazel composition crate preserving the Cargo library's public module paths.
pub use skill_bom_foundation::{config, domain, env, net, paths, process, store};
pub use skill_bom_sources::{archive, clawhub, git, sources};

#[path = "application.rs"]
pub mod application;
#[path = "bom.rs"]
pub mod bom;
#[path = "installer.rs"]
pub mod installer;
#[path = "interfaces.rs"]
pub mod interfaces;
#[path = "resolver.rs"]
pub mod resolver;
