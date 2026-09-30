//! Pure Agent Skills contracts and portable discovery names.
use super::{Error, Result, fail, validate_relative};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Agent {
    #[default]
    Universal,
    Codex,
    ClaudeCode,
    Cursor,
    Relayagent,
}
impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Universal => "universal",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::Cursor => "cursor",
            Self::Relayagent => "relayagent",
        }
    }
    pub fn directory(self, global: bool) -> &'static str {
        match self {
            Self::Universal | Self::Codex => ".agents/skills",
            Self::ClaudeCode => ".claude/skills",
            Self::Cursor => ".cursor/skills",
            Self::Relayagent if global => ".relay/skills",
            Self::Relayagent => ".skills",
        }
    }
    pub fn validate_name(self, name: &str) -> Result<()> {
        if self == Self::ClaudeCode && matches!(name, "synced" | "anthropic-skills") {
            return fail("AGENT_SKILL_NAME", "Claude Code reserves this Skill name");
        }
        Ok(())
    }
}
impl std::str::FromStr for Agent {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "universal" => Ok(Self::Universal),
            "codex" => Ok(Self::Codex),
            "claude-code" => Ok(Self::ClaudeCode),
            "cursor" => Ok(Self::Cursor),
            "relayagent" => Ok(Self::Relayagent),
            _ => Err(Error::new("CONFIG", "Unknown Agent preset", 2)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}
impl SkillFrontmatter {
    pub fn validate(mut self) -> Result<Self> {
        self.name = skill_name(&self.name)?;
        if self.description.trim().is_empty() || self.description.chars().count() > 1024 {
            return fail("SKILL_FORMAT", "description must contain 1–1024 characters");
        }
        if self
            .compatibility
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.chars().count() > 500)
        {
            return fail(
                "SKILL_FORMAT",
                "compatibility must contain 1–500 characters",
            );
        }
        Ok(self)
    }
    pub fn matches_directory(&self, directory: &str) -> Result<()> {
        if self.name != directory {
            return fail(
                "SKILL_NAME_MISMATCH",
                "SKILL.md name differs from the deployment name",
            );
        }
        Ok(())
    }
}
pub fn skill_name(value: &str) -> Result<String> {
    let name: String = value.trim().nfkc().collect();
    if name.is_empty()
        || name.chars().count() > 64
        || name != name.to_lowercase()
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name.chars().all(|c| c.is_alphanumeric() || c == '-')
    {
        return fail(
            "SKILL_FORMAT",
            "name must be 1–64 lowercase letters/digits with single interior hyphens",
        );
    }
    validate_relative(&name)?;
    Ok(name)
}
pub fn directory_key(value: &str) -> String {
    value.nfkc().collect::<String>().to_lowercase()
}
/// Historical names remain readable; new Skill content has the stricter contract.
pub fn deployment_name(value: &str) -> bool {
    super::safe_name(value) || skill_name(value).is_ok_and(|name| name == value)
}
