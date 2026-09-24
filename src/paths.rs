//! Confined portable paths, bounded file I/O, and atomic publication.
use crate::domain::{Error, Result, fail};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub fn relative(s: &str) -> Result<PathBuf> {
    crate::domain::validate_relative(s)?;
    Ok(PathBuf::from(s))
}
pub fn no_symlink(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() && !trusted_macos_alias(&current) => {
                return fail(
                    "UNSAFE_PATH",
                    format!("Symlink path: {}", current.display()),
                );
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn trusted_macos_alias(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let expected = if path == Path::new("/var") {
            Some(Path::new("/private/var"))
        } else if path == Path::new("/tmp") {
            Some(Path::new("/private/tmp"))
        } else {
            None
        };
        expected.is_some_and(|target| std::fs::canonicalize(path).is_ok_and(|p| p == target))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}
pub fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    no_symlink(path)?;
    let f = std::fs::File::open(path)?;
    if !f.metadata()?.is_file() {
        return fail("UNSAFE_PATH", "Expected regular file");
    }
    let mut data = Vec::new();
    f.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err(Error::new(
            "RESOURCE_LIMIT",
            format!("File exceeds {limit} bytes"),
            2,
        ));
    }
    Ok(data)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    no_symlink(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("PATH", "Missing parent", 2))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| Error::from(e.error))?;
    sync_dir(parent)
}
pub fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        crate::env::cwd()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for p in path.components() {
        match p {
            std::path::Component::CurDir => (),
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(p),
        }
    }
    no_symlink(&normalized)?;
    Ok(normalized)
}
