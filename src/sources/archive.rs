//! SHA-pinned HTTPS archive adapter.
use crate::domain::*;
use crate::{net::Http, paths, store};
use std::path::{Path, PathBuf};

pub fn fetch(
    http: &Http,
    url: &str,
    request: &Dependency,
    destination: &Path,
) -> Result<(PathBuf, Evidence)> {
    let bytes = http.get(url, None)?.bytes;
    let hash = digest(&bytes);
    if request
        .sha256
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some(&hash)
    {
        return fail(
            "CHECKSUM_MISMATCH",
            "Archive SHA-256 differs from declaration",
        );
    }
    store::extract(&bytes, destination)?;
    let root = match &request.subdir {
        Some(path) => destination.join(paths::relative(path)?),
        None => destination.to_path_buf(),
    };
    Ok((
        root,
        Evidence {
            archive_sha256: Some(hash),
            ..Evidence::default()
        },
    ))
}
