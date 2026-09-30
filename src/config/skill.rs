//! Bounded, data-only parsing of Agent Skills frontmatter.
use crate::domain::{Error, Result, SkillFrontmatter, fail};

pub const MAX_SKILL_BYTES: u64 = 1024 * 1024;
pub const MAX_FRONTMATTER_BYTES: usize = 64 * 1024;

pub fn parse(bytes: &[u8]) -> Result<SkillFrontmatter> {
    if bytes.len() as u64 > MAX_SKILL_BYTES {
        return Err(Error::new("RESOURCE_LIMIT", "SKILL.md exceeds 1 MiB", 2));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::new("SKILL_FORMAT", "SKILL.md must be UTF-8", 1))?;
    let start = text.find('\n').map_or(text.len(), |offset| offset + 1);
    if text[..start].trim_end_matches(['\r', '\n']) != "---" {
        return fail(
            "SKILL_FORMAT",
            "SKILL.md must start with a --- delimiter line",
        );
    }
    let mut end = start;
    let mut closed = false;
    for line in text[start..].split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            closed = true;
            break;
        }
        end += line.len();
        if end - start > MAX_FRONTMATTER_BYTES {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Frontmatter exceeds 64 KiB",
                2,
            ));
        }
    }
    if !closed {
        return fail("SKILL_FORMAT", "Missing closing --- frontmatter delimiter");
    }
    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_depth: 32,
            flow_nesting_limit: 32,
            max_events: 10_000,
            max_nodes: 5_000,
            max_aliases: 64,
            max_anchors: 64,
            max_recorded_anchor_events: 10_000,
            max_recorded_anchor_bytes: MAX_FRONTMATTER_BYTES,
            max_total_scalar_bytes: MAX_FRONTMATTER_BYTES,
            max_documents: 1,
            max_inclusion_depth: 0,
        },
        alias_limits: serde_saphyr::alias_limits! {
            max_total_replayed_events: 10_000,
            max_replay_stack_depth: 32,
            max_alias_expansions_per_anchor: 64,
        },
        reject_unsupported_tags: true,
        strict_booleans: true,
        with_snippet: false,
    };
    // Parse through a typed value so numbers, booleans and null cannot become strings.
    let value: serde_json::Value = serde_saphyr::from_str_with_options(&text[start..end], options)
        .map_err(|e| {
            let limited = matches!(
                e,
                serde_saphyr::Error::Budget { .. }
                    | serde_saphyr::Error::AliasReplayCounterOverflow { .. }
                    | serde_saphyr::Error::AliasReplayLimitExceeded { .. }
                    | serde_saphyr::Error::AliasExpansionLimitExceeded { .. }
                    | serde_saphyr::Error::AliasReplayStackDepthExceeded { .. }
            );
            Error::new(
                if limited {
                    "RESOURCE_LIMIT"
                } else {
                    "SKILL_FORMAT"
                },
                format!("Invalid frontmatter YAML: {e}"),
                if limited { 2 } else { 1 },
            )
        })?;
    if !value.is_object() {
        return fail("SKILL_FORMAT", "Frontmatter must be a YAML mapping");
    }
    for field in ["license", "compatibility", "metadata", "allowed-tools"] {
        if value.get(field).is_some_and(serde_json::Value::is_null) {
            return fail(
                "SKILL_FORMAT",
                format!("{field} cannot be null when provided"),
            );
        }
    }
    let metadata: SkillFrontmatter = serde_json::from_value(value).map_err(|e| {
        Error::new(
            "SKILL_FORMAT",
            format!("Invalid frontmatter fields: {e}"),
            1,
        )
    })?;
    metadata.validate()
}
