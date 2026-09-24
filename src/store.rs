//! Immutable content-addressed trees. Every read revalidates the content boundary.
use crate::domain::*;
use crate::paths;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub use crate::domain::tree_digest;
pub fn inventory(root: &Path) -> Result<Vec<FileRecord>> {
    paths::no_symlink(root)?;
    if !root.is_dir() {
        return fail("CONTENT_MISSING", "Content directory is missing");
    }
    fn visit(
        root: &Path,
        dir: &Path,
        out: &mut Vec<FileRecord>,
        names: &mut BTreeMap<String, String>,
        total: &mut u64,
        entries: &mut usize,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            *entries += 1;
            if *entries > MAX_FILES * 2 {
                return Err(Error::new("RESOURCE_LIMIT", "Too many tree entries", 2));
            }
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::new("UNSAFE_PATH", "Path escapes root", 1))?;
            let s = relative
                .to_str()
                .ok_or_else(|| Error::new("UNSAFE_PATH", "Non UTF-8 filename", 1))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            paths::relative(&s)?;
            register_path(names, &s)?;
            let m = entry.metadata()?;
            if entry.file_type()?.is_symlink() {
                return fail("UNSAFE_PATH", "Symlinks are forbidden");
            }
            if m.is_dir() {
                visit(root, &path, out, names, total, entries)?;
            } else if m.is_file() {
                *total = total
                    .checked_add(m.len())
                    .ok_or_else(|| Error::new("RESOURCE_LIMIT", "Content size overflow", 2))?;
                if *total > MAX_BYTES || out.len() >= MAX_FILES {
                    return Err(Error::new(
                        "RESOURCE_LIMIT",
                        "Content exceeds file/byte budget",
                        2,
                    ));
                }
                let bytes = paths::read(&path, MAX_BYTES)?;
                if bytes.starts_with(b"version https://git-lfs.github.com/spec/v1") {
                    return fail("LFS_UNMATERIALIZED", "Git LFS pointer is not materialized");
                }
                out.push(FileRecord {
                    path: s,
                    size: bytes.len() as u64,
                    sha256: digest(&bytes),
                    executable: executable(&m),
                });
            } else {
                return fail(
                    "UNSAFE_PATH",
                    "Only regular files and directories are supported",
                );
            }
        }
        Ok(())
    }
    let mut files = vec![];
    visit(root, root, &mut files, &mut BTreeMap::new(), &mut 0, &mut 0)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
fn executable(m: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        false
    }
}
pub fn set_executable(path: &Path, exec: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if exec { 0o755 } else { 0o644 }),
        )?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, exec);
    }
    Ok(())
}
fn register_path(names: &mut BTreeMap<String, String>, path: &str) -> Result<()> {
    let mut prefix = String::new();
    for part in path.split('/') {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        if let Some(old) = names.insert(prefix.to_lowercase(), prefix.clone())
            && old != prefix
        {
            return fail("CASE_COLLISION", format!("{old} and {prefix}"));
        }
    }
    Ok(())
}
pub fn entrypoint(root: &Path) -> Result<PathBuf> {
    let names = ["SKILL.md", "skill.md", "skills.md"];
    let entries = std::fs::read_dir(root)?.collect::<std::io::Result<Vec<_>>>()?;
    let found: Vec<_> = entries
        .into_iter()
        .filter(|entry| names.iter().any(|name| entry.file_name() == *name))
        .collect();
    if found.len() != 1 {
        return fail(
            "SKILL_ENTRYPOINT",
            "Expected exactly one of SKILL.md, skill.md, skills.md",
        );
    }
    let path = found[0].path();
    paths::no_symlink(&path)?;
    if !found[0].file_type()?.is_file() {
        return fail("SKILL_ENTRYPOINT", "Skill entrypoint is not a regular file");
    }
    Ok(path)
}
#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn path(&self, hash: &str) -> Result<PathBuf> {
        if !is_hex(hash, 64) {
            return fail("CACHE_KEY", "Invalid content hash");
        }
        Ok(self.root.join(hash))
    }
    pub fn get(&self, package: &LockedPackage) -> Result<PathBuf> {
        let path = self.path(&package.tree_sha256)?;
        let files = inventory(&path).map_err(|e| {
            Error::new(
                "CACHE_UNAVAILABLE",
                format!("Cached content unavailable: {}", e.code),
                2,
            )
            .hint("Run install online to restore the cache.")
        })?;
        if tree_digest(&files)? != package.tree_sha256 || !same_content(&files, &package.files) {
            return fail("CACHE_CORRUPT", "Cached content differs from lock");
        }
        Ok(path)
    }
    pub fn publish(&self, source: &Path) -> Result<(String, Vec<FileRecord>)> {
        entrypoint(source)?;
        let files = inventory(source)?;
        let hash = tree_digest(&files)?;
        let target = self.path(&hash)?;
        paths::no_symlink(&self.root)?;
        std::fs::create_dir_all(&self.root)?;
        let guard = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join(".publish.lock"))?;
        fs2::FileExt::lock_exclusive(&guard)?;
        if target.exists() {
            if let Ok(existing) = inventory(&target)
                && tree_digest(&existing)? == hash
                && same_content(&existing, &files)
            {
                return Ok((hash, files));
            }
            std::fs::remove_dir_all(&target)?;
        }
        let stage = tempfile::tempdir_in(&self.root)?;
        copy_tree(source, stage.path(), &files)?;
        std::fs::rename(stage.path(), &target)?;
        paths::sync_dir(&self.root)?;
        Ok((hash, files))
    }
}
pub fn same_content(actual: &[FileRecord], expected: &[FileRecord]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, b)| a.path == b.path && a.size == b.size && a.sha256 == b.sha256)
}
pub fn same_files(actual: &[FileRecord], expected: &[FileRecord]) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(a, b)| {
            a.path == b.path
                && a.size == b.size
                && a.sha256 == b.sha256
                && (!cfg!(unix) || a.executable == b.executable)
        })
}
pub fn copy_tree(source: &Path, dest: &Path, files: &[FileRecord]) -> Result<()> {
    paths::no_symlink(dest)?;
    std::fs::create_dir_all(dest)?;
    for f in files {
        let rel = paths::relative(&f.path)?;
        let bytes = paths::read(&source.join(&rel), MAX_BYTES)?;
        if bytes.len() as u64 != f.size || digest(&bytes) != f.sha256 {
            return fail("CONTENT_CHANGED", "Content changed during copy");
        }
        let path = dest.join(rel);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        set_executable(&path, f.executable)?;
    }
    paths::sync_dir(dest)
}

pub fn extract(bytes: &[u8], dest: &Path) -> Result<()> {
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err(Error::new(
            "RESOURCE_LIMIT",
            "Archive exceeds compressed size budget",
            2,
        ));
    }
    let mut budget = ExtractBudget::default();
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(archive_error)?;
        if zip.len() > MAX_FILES * 2 {
            return Err(Error::new("RESOURCE_LIMIT", "Too many archive entries", 2));
        }
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).map_err(archive_error)?;
            let name = file.name().trim_end_matches('/').to_string();
            let mode = file.unix_mode().unwrap_or(0o100644);
            let ty = mode & 0o170000;
            if ty != 0 && ty != 0o100000 && ty != 0o040000 {
                return fail(
                    "ARCHIVE_LINK",
                    "Archive links and special files are forbidden",
                );
            }
            let dir = file.is_dir();
            let size = file.size();
            budget.unpack(dest, &name, size, dir, mode & 0o111 != 0, &mut file)?;
        }
    } else {
        let reader: Box<dyn Read + '_> = if bytes.starts_with(&[0x1f, 0x8b]) {
            Box::new(flate2::read::GzDecoder::new(bytes))
        } else {
            Box::new(bytes)
        };
        let mut tar = tar::Archive::new(reader);
        for entry in tar.entries().map_err(archive_error)? {
            let mut file = entry.map_err(archive_error)?;
            let ty = file.header().entry_type();
            if ty.is_pax_global_extensions() {
                budget.entries += 1;
                if file.size() > 65536 || budget.entries > MAX_FILES * 2 {
                    return Err(Error::new(
                        "RESOURCE_LIMIT",
                        "PAX metadata exceeds budget",
                        2,
                    ));
                }
                let extensions = file
                    .pax_extensions()
                    .map_err(archive_error)?
                    .ok_or_else(|| archive_error("Missing PAX extensions"))?;
                for extension in extensions {
                    let extension = extension.map_err(archive_error)?;
                    let key = extension.key().map_err(archive_error)?;
                    if !["comment", "mtime", "atime", "ctime"].contains(&key) {
                        return fail("ARCHIVE", "Unsupported global PAX directive");
                    }
                }
                continue;
            }
            if !ty.is_file() && !ty.is_dir() {
                return fail(
                    "ARCHIVE_LINK",
                    "Archive links and special files are forbidden",
                );
            }
            let name = std::str::from_utf8(&file.path_bytes())
                .map_err(archive_error)?
                .trim_end_matches('/')
                .to_string();
            let size = file.size();
            let exec = file.header().mode().map_err(archive_error)? & 0o111 != 0;
            budget.unpack(dest, &name, size, ty.is_dir(), exec, &mut file)?;
        }
    }
    Ok(())
}
fn archive_error(e: impl std::fmt::Display) -> Error {
    Error::new("ARCHIVE", e.to_string(), 1).phase("extraction")
}
#[derive(Default)]
struct ExtractBudget {
    entries: usize,
    bytes: u64,
    names: BTreeMap<String, String>,
    files: BTreeSet<String>,
}
impl ExtractBudget {
    fn unpack(
        &mut self,
        dest: &Path,
        name: &str,
        size: u64,
        dir: bool,
        exec: bool,
        reader: &mut dyn Read,
    ) -> Result<()> {
        self.entries += 1;
        self.bytes = self
            .bytes
            .checked_add(size)
            .ok_or_else(|| Error::new("RESOURCE_LIMIT", "Archive size overflow", 2))?;
        if self.entries > MAX_FILES * 2 || self.bytes > MAX_BYTES || self.files.len() >= MAX_FILES {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Archive exceeds extraction budget",
                2,
            ));
        }
        let rel = paths::relative(name)?;
        register_path(&mut self.names, name)?;
        let path = dest.join(rel);
        if dir {
            std::fs::create_dir_all(path)?;
            return Ok(());
        }
        if !self.files.insert(name.into()) {
            return fail("ARCHIVE_DUPLICATE", format!("Duplicate file {name}"));
        }
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut out = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        let copied = std::io::copy(&mut reader.take(size + 1), &mut out)?;
        if copied != size {
            return fail("ARCHIVE_SIZE", "Archive entry size mismatch");
        }
        set_executable(&path, exec)?;
        Ok(())
    }
}
