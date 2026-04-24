//! Filesystem primitives: atomic writes and safe reads.
//!
//! All entity files are written via [`atomic_write_bytes`]. The
//! procedure (see `docs/DATA_MODEL.md §3`):
//!
//! 1. Write bytes to `<dest>.tmp.<nonce>` inside the same directory.
//! 2. `fsync` the tmp file.
//! 3. Rename tmp → dest (atomic on POSIX).
//! 4. `fsync` the destination's parent directory so the rename persists.
//!
//! Step 4 is frequently omitted by naive implementations; we do it
//! because a crash between (3) and (4) can otherwise leave the rename
//! invisible after reboot.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{StoreError, StoreResult};

/// Read a file to bytes. Returns `Ok(None)` when the file is missing —
/// this is the common "entity does not exist" case and shouldn't force
/// callers to match on `ErrorKind::NotFound`.
pub fn read_optional_bytes(path: &Path) -> StoreResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StoreError::io(path, e)),
    }
}

/// Read a file to bytes, failing with `NotFound` when missing.
pub fn read_bytes(path: &Path) -> StoreResult<Vec<u8>> {
    read_optional_bytes(path)?
        .ok_or_else(|| StoreError::io(path, std::io::Error::from(std::io::ErrorKind::NotFound)))
}

/// Atomically replace the contents of `dest` with `bytes`.
///
/// Creates missing parent directories. The temp file is always cleaned
/// up — either by being renamed into place on success, or by explicit
/// removal on failure.
pub fn atomic_write_bytes(dest: &Path, bytes: &[u8]) -> StoreResult<()> {
    let parent = dest.parent().ok_or_else(|| {
        StoreError::io(
            dest,
            std::io::Error::other("destination has no parent directory"),
        )
    })?;
    fs::create_dir_all(parent).map_err(|e| StoreError::io(parent, e))?;

    let tmp = tmp_sibling(dest);
    let result = (|| -> StoreResult<()> {
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)
                .map_err(|e| StoreError::io(&tmp, e))?;
            f.write_all(bytes).map_err(|e| StoreError::io(&tmp, e))?;
            f.sync_all().map_err(|e| StoreError::io(&tmp, e))?;
        }
        fs::rename(&tmp, dest).map_err(|e| StoreError::io(dest, e))?;
        // Directory fsync so the rename is durable across crashes.
        fsync_dir(parent)?;
        Ok(())
    })();

    if result.is_err() {
        // Best-effort cleanup. Ignore the nested error; the primary
        // error is far more interesting.
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Write a UTF-8 string atomically.
pub fn atomic_write_str(dest: &Path, s: &str) -> StoreResult<()> {
    atomic_write_bytes(dest, s.as_bytes())
}

/// Remove a file, treating "already gone" as success.
pub fn remove_if_exists(path: &Path) -> StoreResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StoreError::io(path, e)),
    }
}

/// Remove an entire directory tree, treating "already gone" as success.
pub fn remove_dir_if_exists(path: &Path) -> StoreResult<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StoreError::io(path, e)),
    }
}

/// Return a sibling tmp path inside the same directory. Keeping the
/// tmp file in the same directory is a prerequisite for `rename` to be
/// atomic (cross-filesystem rename would copy + unlink, breaking the
/// atomicity guarantee).
fn tmp_sibling(dest: &Path) -> PathBuf {
    // Nonce is process id + monotonic counter. Sufficient to avoid
    // collisions between concurrent writers in the same process; across
    // processes a crash-leftover tmp file will simply be overwritten on
    // the next write.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let base = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".{base}.tmp.{pid}.{seq}"))
}

/// `fsync` a directory so renames inside it become durable.
fn fsync_dir(dir: &Path) -> StoreResult<()> {
    // On Unix, `File::open` gives us an fd; `sync_all` on a directory
    // fd forces the rename to persist to disk.
    match File::open(dir) {
        Ok(f) => f.sync_all().map_err(|e| StoreError::io(dir, e)),
        // On Windows there's no meaningful directory sync and opening
        // with `File::open` fails — accept that as a no-op, matching
        // how other crates (`tempfile`, `fs2`) behave.
        Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(e) => Err(StoreError::io(dir, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn atomic_write_creates_file_and_parents() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a/b/c/file.txt");
        atomic_write_str(&nested, "hello").unwrap();
        assert_eq!(fs::read_to_string(&nested).unwrap(), "hello");
    }

    #[test]
    fn atomic_write_replaces_existing_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("x.toml");
        atomic_write_str(&path, "v1").unwrap();
        atomic_write_str(&path, "v2").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2");
    }

    #[test]
    fn read_optional_missing_is_none() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nope.toml");
        assert!(read_optional_bytes(&path).unwrap().is_none());
    }

    #[test]
    fn read_bytes_missing_is_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nope.toml");
        assert!(read_bytes(&path).is_err());
    }

    #[test]
    fn remove_if_exists_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("gone.toml");
        remove_if_exists(&path).unwrap();
        atomic_write_str(&path, "here").unwrap();
        remove_if_exists(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn no_tmp_files_leaked_on_success() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("clean.toml");
        atomic_write_str(&path, "ok").unwrap();
        let leftover: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftover.is_empty(), "leaked tmp files: {leftover:?}");
    }

    #[test]
    fn no_tmp_files_leaked_on_failure() {
        // Make the destination directory read-only so the final rename
        // fails; assert the tmp sibling is cleaned up.
        let dir = TempDir::new().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        // Pre-place a file where we're about to write.
        let target = locked.join("file.toml");
        atomic_write_str(&target, "v1").unwrap();
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&locked, perms).unwrap();

        let result = atomic_write_str(&target, "v2");
        // Reset perms to clean up the tempdir.
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(&locked, perms);

        if result.is_err() {
            let leftover: Vec<_> = fs::read_dir(&locked)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name().into_string().unwrap())
                .filter(|n| n.contains(".tmp."))
                .collect();
            assert!(leftover.is_empty(), "leaked tmp on failure: {leftover:?}");
        }
        // Else: on this platform the write unexpectedly succeeded; not
        // fatal, just means the test didn't exercise the failure path.
    }

    #[test]
    fn concurrent_writers_converge_to_one_content() {
        // Two threads racing `atomic_write_str` on the same path must
        // end with the file containing *one* of the two values
        // (never mixed, never corrupt), and no leftover tmp files.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("race.toml");
        let path_a = path.clone();
        let path_b = path.clone();
        let ta = thread::spawn(move || {
            for _ in 0..200 {
                atomic_write_str(&path_a, "AAAA").unwrap();
            }
        });
        let tb = thread::spawn(move || {
            for _ in 0..200 {
                atomic_write_str(&path_b, "BBBB").unwrap();
            }
        });
        ta.join().unwrap();
        tb.join().unwrap();
        let final_content = fs::read_to_string(&path).unwrap();
        assert!(matches!(final_content.as_str(), "AAAA" | "BBBB"));
        let leftover: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(
            leftover.is_empty(),
            "leaked tmp under contention: {leftover:?}"
        );
    }
}
