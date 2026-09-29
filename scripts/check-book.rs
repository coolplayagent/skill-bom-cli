//! Check local HTML links and assets in the generated mdBook before publishing.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

fn html_files(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            html_files(&entry.path(), files)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "html") {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("usage: check-book BOOK_DIR")?;
    let root = PathBuf::from(directory).canonicalize()?;
    if !root.join("index.html").is_file() {
        return Err("book index.html is missing".into());
    }
    let mut files = Vec::new();
    html_files(&root, &mut files)?;
    let mut checked = 0;
    let mut broken = 0;
    for file in &files {
        let html = std::fs::read_to_string(file)?;
        for attribute in ["href=\"", "src=\""] {
            for tail in html.split(attribute).skip(1) {
                let target = tail.split_once('"').ok_or("unclosed HTML attribute")?.0;
                if target.contains(':') || target.starts_with("//") {
                    continue;
                }
                let path = target.split(['#', '?']).next().unwrap_or_default();
                if path.is_empty() {
                    continue;
                }
                let mut destination = if let Some(relative) = path.strip_prefix("/skill-bom-cli/") {
                    root.join(relative)
                } else {
                    file.parent().ok_or("HTML file has no parent")?.join(path)
                };
                if destination.is_dir() {
                    destination.push("index.html");
                }
                checked += 1;
                if !destination.is_file() || !destination.canonicalize()?.starts_with(&root) {
                    eprintln!(
                        "{}: broken local link {target}",
                        file.strip_prefix(&root)?.display()
                    );
                    broken += 1;
                }
            }
        }
    }
    if broken > 0 {
        return Err(format!("{broken} broken links in {} HTML files", files.len()).into());
    }
    println!(
        "Checked {checked} local links/assets in {} HTML files",
        files.len()
    );
    Ok(())
}
