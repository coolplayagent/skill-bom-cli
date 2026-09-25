use crate::domain::*;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "skill-bom",
    version,
    about = "Resolve, install and audit Skill dependencies without executing Skill content"
)]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH", conflicts_with = "global")]
    pub manifest: Option<PathBuf>,
    #[arg(long, global = true)]
    pub global: bool,
    #[arg(long, global = true, value_name = "PATH")]
    pub target: Option<PathBuf>,
    #[arg(long, global = true)]
    pub offline: bool,
    #[arg(long, global = true)]
    pub strict_metadata: bool,
    #[arg(long, global = true, value_enum, default_value = "text")]
    pub format: Format,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    SpdxJson,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a minimal skills.toml without overwriting an existing file.
    Init,
    /// Validate TOML, source combinations and root registry references.
    Validate,
    /// Resolve dependencies and write skills.lock without deploying.
    Lock,
    /// Update all packages or one root alias, retaining other versions when possible.
    Update { alias: Option<String> },
    /// Update allowed versions and deploy the resolved graph in one operation.
    Sync {
        alias: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Verify and deploy locked content using a recoverable transaction.
    Install {
        #[arg(long)]
        locked: bool,
        #[arg(long)]
        frozen: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Display the locked dependency graph.
    Tree,
    /// Show root-to-package introduction paths (identity, alias or installed name).
    Why { package: String },
    /// List installation records in the current scope.
    List,
    /// Detect missing, modified and out-of-date installed content.
    Verify,
    /// Export a lock or verified installation view; no network access.
    Bom {
        #[arg(long = "from", value_enum, default_value = "lock")]
        from: BomFrom,
        #[arg(long, value_name = "RFC3339")]
        timestamp: Option<String>,
    },
    /// Export a versioned JSON Schema for tooling.
    Schema {
        #[arg(value_enum)]
        kind: SchemaKind,
    },
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BomFrom {
    Lock,
    Installed,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SchemaKind {
    Manifest,
    Package,
    Lock,
    Bom,
    Installed,
    Error,
}
pub struct Output {
    pub value: serde_json::Value,
    pub text: Option<String>,
    pub exit_code: u8,
}
impl Output {
    pub fn json(value: impl serde::Serialize) -> Result<Self> {
        Ok(Self {
            value: serde_json::to_value(value)?,
            text: None,
            exit_code: 0,
        })
    }
}
pub fn render(output: Output, format: Format) -> Result<u8> {
    let text = if format == Format::Text {
        output
            .text
            .unwrap_or(serde_json::to_string_pretty(&output.value)?)
    } else {
        serde_json::to_string_pretty(&output.value)?
    };
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{text}")
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(e.into());
    }
    Ok(output.exit_code)
}
pub fn error(error: &Error, json: bool) {
    if json {
        eprintln!("{}", serde_json::json!({"error":error}));
    } else {
        eprintln!("{error}\n{}", error.hint);
        for chain in &error.chains {
            eprintln!("  {}", chain.join(" -> "));
        }
    }
}
