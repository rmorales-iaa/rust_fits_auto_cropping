//! Best-effort persistence barriers for files and their directory entries.
use std::{fs::File, io, path::Path};

/// Persist the directory entry containing a newly published or removed path.
pub fn sync_parent(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "published path has no parent")
    })?;
    sync_directory(parent)
}

/// Persist a directory entry update where the platform exposes directory fsync.
#[cfg(unix)]
pub fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

/// `std` does not expose directory fsync on non-Unix platforms.
#[cfg(not(unix))]
pub fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncs_a_temporary_directory_and_parent() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("product.fits");
        File::create(&file).unwrap().sync_all().unwrap();
        sync_parent(&file).unwrap();
        sync_directory(root.path()).unwrap();
    }

    #[test]
    fn rejects_paths_without_a_parent() {
        assert!(sync_parent(Path::new("product.fits")).is_err());
    }
}
