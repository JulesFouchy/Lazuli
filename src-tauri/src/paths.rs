//! Filename collision handling.
//!
//! Nothing in a project folder is ever overwritten. When a name is taken, the
//! incoming file gets a ` (2)`, ` (3)`, ... suffix and both files survive. Every
//! write that puts a file into a project folder goes through [`unique_path`].

use std::path::{Path, PathBuf};

/// Highest suffix tried before giving up, so a pathological directory cannot
/// spin forever.
const MAX_ATTEMPTS: u32 = 10_000;

/// A path inside `dir` for `filename` that does not collide with anything
/// already there.
///
/// Returns `dir/filename` untouched when it is free, otherwise inserts a
/// counter before the extension: `photo.jpg` becomes `photo (2).jpg`.
///
/// This is inherently racy against another process creating the same name in
/// the window between the check and the write. That is accepted: the competing
/// writer is a human in Explorer, and the watcher reconciles whatever ends up
/// on disk.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let (stem, extension) = split_extension(filename);
    for n in 2..MAX_ATTEMPTS {
        let candidate = dir.join(format!("{stem} ({n}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    // Astronomically unlikely. Fall back to something unmistakably unique
    // rather than returning a path that would overwrite a real file.
    dir.join(format!("{stem} ({}){extension}", uuid::Uuid::new_v4()))
}

/// Split a filename into its stem and its extension *including* the dot.
///
/// Hand-rolled rather than using `Path::extension` so that dotfiles and
/// multi-dot names keep their full visible name in the stem: `.gitignore` has
/// no extension, and `archive.tar.gz` yields `archive.tar` + `.gz`.
fn split_extension(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        // A leading dot is part of the name, not an extension separator.
        Some(0) | None => (filename, ""),
        Some(index) => filename.split_at(index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A scratch directory that removes itself when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("journaley-{label}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("should be able to create a temp dir");
            Self(path)
        }

        fn touch(&self, name: &str) {
            fs::write(self.0.join(name), b"x").expect("should be able to write a test file");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_free_name_is_returned_unchanged() {
        let dir = TempDir::new("free");
        assert_eq!(unique_path(&dir.0, "photo.jpg"), dir.0.join("photo.jpg"));
    }

    #[test]
    fn a_taken_name_gets_a_suffix_before_the_extension() {
        let dir = TempDir::new("taken");
        dir.touch("photo.jpg");
        assert_eq!(unique_path(&dir.0, "photo.jpg"), dir.0.join("photo (2).jpg"));
    }

    #[test]
    fn suffixes_keep_climbing() {
        let dir = TempDir::new("climb");
        dir.touch("photo.jpg");
        dir.touch("photo (2).jpg");
        dir.touch("photo (3).jpg");
        assert_eq!(unique_path(&dir.0, "photo.jpg"), dir.0.join("photo (4).jpg"));
    }

    #[test]
    fn names_without_an_extension_still_work() {
        let dir = TempDir::new("noext");
        dir.touch("README");
        assert_eq!(unique_path(&dir.0, "README"), dir.0.join("README (2)"));
    }

    #[test]
    fn only_the_last_extension_is_split_off() {
        assert_eq!(split_extension("archive.tar.gz"), ("archive.tar", ".gz"));
    }

    #[test]
    fn a_leading_dot_is_part_of_the_name() {
        // Otherwise `.gitignore` would collide as ` (2).gitignore`.
        assert_eq!(split_extension(".gitignore"), (".gitignore", ""));
    }
}
