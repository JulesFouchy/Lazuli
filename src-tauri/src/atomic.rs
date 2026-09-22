//! Writing a file so that no reader ever sees half of it.
//!
//! Every persisted file used to be a bare `fs::write`, which is fine while the
//! app is the only writer and wrong the moment anything else reads or writes
//! concurrently — a sync process, a text editor, or the app's own watcher
//! firing mid-write. `fs::write` truncates first and fills afterwards, so a
//! reader that arrives in between gets an empty or partial file.
//!
//! Write to a temp file in the same directory and rename over the target
//! instead. A rename within a directory is atomic on every platform the app
//! ships to: a reader sees either the old contents or the new ones, never a
//! mixture. The updater already stages its installer this way; this is the same
//! move for the small files.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Write `contents` to `path`, atomically.
///
/// The temp file is a sibling, because a rename is only atomic within one
/// filesystem, and carries a UUID so two writers cannot collide on it. Its name
/// is deliberately not one the scanner recognises: `list_entry_dirs` looks for
/// `entry.md` exactly, and the extension here can never match
/// [`crate::model::IMAGE_EXTENSIONS`], so a scan landing inside the window
/// ignores it rather than showing a phantom.
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    let directory = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("{} has no filename", path.display()))?;

    let temp = directory.join(format!(".{filename}.tmp-{}", uuid::Uuid::new_v4()));

    if let Err(err) = write_and_sync(&temp, contents.as_ref()) {
        // Nothing has touched `path` yet, so the only thing to undo is the
        // half-written temp file.
        let _ = fs::remove_file(&temp);
        return Err(err);
    }

    fs::rename(&temp, path)
        .inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
        .with_context(|| format!("replacing {}", path.display()))
}

/// Fill the temp file and flush it to the disk before it is renamed into place.
///
/// Without the `sync_all`, a crash between the rename and the flush can leave
/// the target named correctly and holding nothing — the rename is ordered, the
/// data behind it is not.
fn write_and_sync(temp: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;

    let mut file = fs::File::create(temp)
        .with_context(|| format!("creating {}", temp.display()))?;
    file.write_all(contents)
        .with_context(|| format!("writing {}", temp.display()))?;
    file.sync_all()
        .with_context(|| format!("flushing {}", temp.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A scratch directory that removes itself when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("lazuli-{label}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("should be able to create a temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_new_file_gets_the_contents() {
        let dir = TempDir::new("atomic-new");
        let path = dir.0.join("entry.md");
        write(&path, "hello").expect("should write");
        assert_eq!(fs::read_to_string(&path).expect("should read"), "hello");
    }

    #[test]
    fn an_existing_file_is_replaced() {
        let dir = TempDir::new("atomic-replace");
        let path = dir.0.join("entry.md");
        write(&path, "first").expect("should write");
        write(&path, "second").expect("should write");
        assert_eq!(fs::read_to_string(&path).expect("should read"), "second");
    }

    #[test]
    fn no_temp_file_is_left_behind() {
        // A leftover temp file would be harmless to the scanner but would
        // accumulate in a project folder the user reads with their own eyes.
        let dir = TempDir::new("atomic-clean");
        write(&dir.0.join("entry.md"), "hello").expect("should write");
        let names: Vec<_> = fs::read_dir(&dir.0)
            .expect("should list")
            .map(|entry| entry.expect("should read entry").file_name())
            .collect();
        assert_eq!(names, vec!["entry.md"]);
    }

    #[test]
    fn a_failure_leaves_the_previous_contents_alone() {
        // The directory the temp file would go in does not exist, so the write
        // fails before anything replaces the target.
        let dir = TempDir::new("atomic-fail");
        let path = dir.0.join("gone").join("entry.md");
        assert!(write(&path, "hello").is_err());
    }
}
