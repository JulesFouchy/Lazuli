//! Who wrote an entry, and how a name reaches whoever reads it.
//!
//! An entry records an author *id*, not a name, because a name changes and an
//! entry should not have to be rewritten when it does. The id is resolved
//! through a folder inside the project:
//!
//! ```text
//! <project>/authors/<author-uuid>/profile.yaml
//! ```
//!
//! That folder is a *publication* of the global profile in the app's own
//! settings, not a setting of its own. The profile is edited in one place and
//! copied into each project the user writes to, because a collaborator opening
//! the folder can read neither our settings nor our account — the name has to
//! be in the folder or it is not anywhere.
//!
//! **Each person only ever writes their own folder**, which is what makes the
//! registry conflict-free when two people sync the same project: the same
//! property that makes UUID entry folders safe.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::atomic;

/// The folder inside a project that holds one record per author.
pub const AUTHORS_DIR: &str = "authors";

const PROFILE_FILE: &str = "profile.yaml";

/// What a project records about one author.
///
/// Only a name so far. An avatar and the account ids that let a second device
/// recognise its own author belong here too, and are left out until there is
/// something that sets them — the file gains fields the way `lazuli.yaml` has,
/// with `#[serde(default)]` keeping the older ones readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
}

/// Every author a project knows, by id.
pub fn read_all(root: &Path) -> HashMap<String, Profile> {
    let Ok(entries) = fs::read_dir(root.join(AUTHORS_DIR)) else {
        return HashMap::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_owned();
            Some((id, read_profile(&entry.path()).ok()?))
        })
        .collect()
}

/// Write this user's profile into the project, if it is not already there and
/// the same.
///
/// Called when the user *writes* to a project rather than when they open one: a
/// project only read has no reason to gain a folder naming whoever looked at
/// it. Rewriting only on a difference keeps it from being a write on every
/// edit, which the watcher would rescan for.
///
/// Failure is not an error worth stopping an edit for — a project someone
/// shared read-only cannot be written to at all, and the edit that triggered
/// this has already succeeded or failed on its own terms.
pub fn publish(root: &Path, id: &str, profile: &Profile) {
    let folder = root.join(AUTHORS_DIR).join(id);
    if read_profile(&folder).is_ok_and(|existing| &existing == profile) {
        return;
    }
    if fs::create_dir_all(&folder).is_err() {
        return;
    }
    if let Ok(yaml) = serde_yaml::to_string(profile) {
        let _ = atomic::write(&folder.join(PROFILE_FILE), yaml);
    }
}

fn read_profile(folder: &Path) -> Result<Profile> {
    let path = folder.join(PROFILE_FILE);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml::from_str(&contents).with_context(|| format!("parsing {}", path.display()))
}

/// A name to start someone off with, taken from the machine.
///
/// Better than "Author" and better than asking on first launch: a journal is
/// solo until it is not, and the name does not matter until it is shared, by
/// which time there is somewhere to change it.
pub fn name_from_the_machine() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Me".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

    fn profile(name: &str) -> Profile {
        Profile {
            name: name.to_owned(),
        }
    }

    #[test]
    fn a_published_profile_reads_back() {
        let dir = TempDir::new("authors-publish");
        publish(&dir.0, "author-1", &profile("Jules"));
        assert_eq!(read_all(&dir.0).get("author-1"), Some(&profile("Jules")));
    }

    #[test]
    fn a_project_with_no_authors_folder_reads_as_empty() {
        let dir = TempDir::new("authors-none");
        assert!(read_all(&dir.0).is_empty());
    }

    #[test]
    fn publishing_the_same_profile_again_does_not_rewrite_it() {
        // Every write is a filesystem event the watcher rescans for, and this
        // runs on every edit.
        let dir = TempDir::new("authors-idempotent");
        publish(&dir.0, "author-1", &profile("Jules"));
        let path = dir.0.join(AUTHORS_DIR).join("author-1").join(PROFILE_FILE);
        let first = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .expect("should stat");

        publish(&dir.0, "author-1", &profile("Jules"));
        let second = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .expect("should stat");
        assert_eq!(first, second);
    }

    #[test]
    fn a_changed_name_is_published_over_the_old_one() {
        let dir = TempDir::new("authors-rename");
        publish(&dir.0, "author-1", &profile("Jules"));
        publish(&dir.0, "author-1", &profile("Jules F"));
        assert_eq!(read_all(&dir.0).get("author-1"), Some(&profile("Jules F")));
    }

    #[test]
    fn several_authors_live_side_by_side() {
        // Each person writes only their own folder, which is what keeps the
        // registry free of conflicts when two of them sync one project.
        let dir = TempDir::new("authors-several");
        publish(&dir.0, "author-1", &profile("Jules"));
        publish(&dir.0, "author-2", &profile("Manu"));
        let all = read_all(&dir.0);
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("author-2"), Some(&profile("Manu")));
    }

    #[test]
    fn a_name_is_never_empty() {
        assert!(!name_from_the_machine().is_empty());
    }
}
