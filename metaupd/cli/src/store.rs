//! File handling for metaupd's private directories.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Where metaupd keeps its data.
#[derive(Debug, Clone)]
pub struct Paths {
    pub state_dir: PathBuf,
    pub trust_dir: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            state_dir: PathBuf::from("/var/db/metaupd"),
            trust_dir: PathBuf::from("/usr/local/etc/meta/trust"),
        }
    }
}

impl Paths {
    pub fn staging(&self) -> PathBuf {
        self.state_dir.join("staging")
    }

    pub fn root_chain(&self) -> PathBuf {
        self.state_dir.join("trust")
    }

    pub fn history(&self) -> PathBuf {
        self.state_dir.join("history.jsonl")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{} is writable by other users or owned by another user; refusing to use it", .0.display())]
    Unsafe(PathBuf),
    #[error("another metaupd process is running")]
    Locked,
    #[error("{} is corrupt: {reason}", path.display())]
    Corrupt { path: PathBuf, reason: String },
}

pub(crate) fn io_at(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Creates `path` if needed and refuses it if other users could modify it.
pub fn ensure_private_dir(path: &Path) -> Result<(), StoreError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o755);
    }
    builder.create(path).map_err(io_at(path))?;
    check_not_shared(path)
}

/// Requires `path` to be owned by root or the current user and not writable by
/// group or others. Ownership is not checked on non-Unix development hosts.
pub fn check_not_shared(path: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).map_err(io_at(path))?;
        let owner_ok = metadata.uid() == 0 || metadata.uid() == rustix::process::geteuid().as_raw();
        if !owner_ok || metadata.mode() & 0o022 != 0 {
            return Err(StoreError::Unsafe(path.to_owned()));
        }
    }
    #[cfg(not(unix))]
    fs::metadata(path).map_err(io_at(path))?;
    Ok(())
}

/// Reads at most `limit` bytes; `None` when the file does not exist.
pub fn read_limited(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, StoreError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_at(path)(e)),
    };
    let mut data = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(io_at(path))?;
    if data.len() as u64 > limit {
        return Err(StoreError::Corrupt {
            path: path.to_owned(),
            reason: "too large".into(),
        });
    }
    Ok(Some(data))
}

/// Replaces `path` so that a crash leaves either the old or the new contents.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".new");
    let temporary = PathBuf::from(temporary);
    let mut file = File::create(&temporary).map_err(io_at(&temporary))?;
    file.write_all(data)
        .and_then(|()| file.sync_all())
        .map_err(io_at(&temporary))?;
    drop(file);
    fs::rename(&temporary, path).map_err(io_at(path))?;
    sync_parent(path)
}

pub(crate) fn sync_parent(path: &Path) -> Result<(), StoreError> {
    // Directories cannot be opened for syncing on Windows development hosts.
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(io_at(parent))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Held for the duration of any operation that changes metaupd's state.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

impl Lock {
    pub fn acquire(state_dir: &Path) -> Result<Self, StoreError> {
        ensure_private_dir(state_dir)?;
        let path = state_dir.join("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(io_at(&path))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(fs::TryLockError::WouldBlock) => Err(StoreError::Locked),
            Err(fs::TryLockError::Error(source)) => Err(StoreError::Io { path, source }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(read_limited(&path, 10).unwrap().unwrap(), b"two");
        assert!(read_limited(&path, 2).is_err());
        assert!(
            read_limited(&dir.path().join("missing"), 2)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let held = Lock::acquire(&state).unwrap();
        assert!(matches!(Lock::acquire(&state), Err(StoreError::Locked)));
        drop(held);
        Lock::acquire(&state).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn group_writable_directories_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o775)).unwrap();
        assert!(matches!(
            ensure_private_dir(dir.path()),
            Err(StoreError::Unsafe(_))
        ));
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(dir.path()).unwrap();
    }
}
