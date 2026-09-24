//! Isolated Bazel crate for the AgentCenter source adapter.
pub use skill_bom_foundation::{config, domain, env, net, paths, store};

#[path = "agentcenter.rs"]
pub mod agentcenter;
