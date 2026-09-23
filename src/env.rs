//! Central environment and clock access.
use crate::domain::{Error, Result};
use std::path::PathBuf;
pub fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok()
}
pub fn cwd() -> Result<PathBuf> {
    Ok(std::env::current_dir()?)
}
pub struct UserDirs {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
}
impl UserDirs {
    pub fn config_dir(&self) -> &std::path::Path {
        &self.config
    }
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data
    }
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache
    }
}
pub fn directories() -> Result<UserDirs> {
    if let Some(root) = variable("SKILL_BOM_HOME") {
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            return Err(Error::new(
                "USER_DIRECTORY",
                "SKILL_BOM_HOME must be absolute",
                2,
            ));
        }
        return Ok(UserDirs {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
        });
    }
    directories::ProjectDirs::from("org", "skill-bom", "skill-bom")
        .map(|d| UserDirs {
            config: d.config_dir().into(),
            data: d.data_dir().into(),
            cache: d.cache_dir().into(),
        })
        .ok_or_else(|| Error::new("USER_DIRECTORY", "Cannot determine user directories", 2))
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
pub fn timestamp(explicit: Option<&str>) -> Result<String> {
    if let Some(s) = explicit {
        return chrono::DateTime::parse_from_rfc3339(s)
            .map(|t| {
                t.with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            })
            .map_err(|_| Error::new("TIMESTAMP", "Expected RFC3339 timestamp", 2));
    }
    if let Some(s) = variable("SOURCE_DATE_EPOCH") {
        let seconds = s
            .parse::<i64>()
            .map_err(|_| Error::new("TIMESTAMP", "Invalid SOURCE_DATE_EPOCH", 2))?;
        return chrono::DateTime::from_timestamp(seconds, 0)
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            .ok_or_else(|| Error::new("TIMESTAMP", "Timestamp out of range", 2));
    }
    Ok(now())
}

pub fn arguments() -> Vec<std::ffi::OsString> {
    std::env::args_os().collect()
}
