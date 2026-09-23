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
/// Every field is defaulted, so a record written before one existed still
/// reads — the same rule `lazuli.yaml` follows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// The picture's filename, beside this file. A *copy* of the one in the
    /// user's settings, because a collaborator can reach the project folder and
    /// nothing else of ours.
    #[serde(default)]
    pub avatar: Option<String>,
    /// Accounts this author signs in with, as `<backend>:<id>`.
    ///
    /// How a second device works out that it is *this* author rather than a new
    /// one. Nothing carries an author id between machines — the settings that
    /// hold it are per machine by design — so the first sync looks its own
    /// account up here and adopts what it finds. Without that a phone and a
    /// laptop are two collaborators, and every solo project starts showing
    /// names.
    #[serde(default)]
    pub accounts: Vec<String>,
    /// What to call this person *here*, when it differs from their own name.
    ///
    /// The Discord model: the name is yours everywhere until you decide it is
    /// not, and then only in the project you decided it in. Absent, [`Self::name`]
    /// shows — and a later rename of the global name reaches every project that
    /// has no override, and none that has.
    #[serde(default)]
    pub display_name: Option<String>,
}

impl Profile {
    /// What a card should call this author.
    pub fn shown_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }

    /// Whether this record belongs to whoever is signed in as `account`.
    pub fn signs_in_as(&self, account: &str) -> bool {
        self.accounts.iter().any(|known| known == account)
    }
}

/// The author id that already belongs to whoever signs in as `account`.
///
/// Called before an entry is written, so that a second device joins the author
/// it finds rather than minting a rival.
pub fn id_for_account(root: &Path, account: &str) -> Option<String> {
    read_all(root)
        .into_iter()
        .find(|(_, profile)| profile.signs_in_as(account))
        .map(|(id, _)| id)
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
pub fn publish(root: &Path, id: &str, name: &str, avatar: Option<&Path>, account: Option<&str>) {
    let folder = root.join(AUTHORS_DIR).join(id);
    let existing = read_profile(&folder).ok();

    // Kept from what is already published rather than passed in. A name shown
    // only here is a decision made here, and republishing the global name must
    // not quietly undo it.
    let display_name = existing.as_ref().and_then(|old| old.display_name.clone());

    // Added to rather than replaced: somebody who signs in with two accounts is
    // still one author, and the account that is not in use today should not be
    // forgotten because of it.
    let mut accounts = existing.map(|old| old.accounts).unwrap_or_default();
    if let Some(account) = account {
        if !accounts.iter().any(|known| known == account) {
            accounts.push(account.to_owned());
        }
    }

    let profile = Profile {
        name: name.to_owned(),
        avatar: avatar
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned),
        accounts,
        display_name,
    };

    // The picture is checked separately from the record: the two are written in
    // two steps, so a run that stopped between them leaves a record naming a
    // file that is not there, and the next one has to finish the job.
    let picture_is_here = profile
        .avatar
        .as_ref()
        .is_none_or(|name| folder.join(name).is_file());
    if picture_is_here && read_profile(&folder).is_ok_and(|existing| existing == profile) {
        return;
    }

    if fs::create_dir_all(&folder).is_err() {
        return;
    }
    if let (Some(source), Some(name)) = (avatar, &profile.avatar) {
        let destination = folder.join(name);
        if !destination.is_file() && fs::copy(source, &destination).is_err() {
            return;
        }
    }
    if let Ok(yaml) = serde_yaml::to_string(&profile) {
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

/// Set, or clear, what this author is called in one project alone.
pub fn set_display_name(root: &Path, id: &str, name: Option<&str>) -> Result<()> {
    let folder = root.join(AUTHORS_DIR).join(id);
    let mut profile = read_profile(&folder)?;
    profile.display_name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    let yaml = serde_yaml::to_string(&profile).context("serialising a profile")?;
    atomic::write(&folder.join(PROFILE_FILE), yaml)
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
            avatar: None,
            accounts: Vec::new(),
            display_name: None,
        }
    }

    #[test]
    fn a_published_profile_reads_back() {
        let dir = TempDir::new("authors-publish");
        publish(&dir.0, "author-1", "Jules", None, None);
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
        publish(&dir.0, "author-1", "Jules", None, None);
        let path = dir.0.join(AUTHORS_DIR).join("author-1").join(PROFILE_FILE);
        let first = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .expect("should stat");

        publish(&dir.0, "author-1", "Jules", None, None);
        let second = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .expect("should stat");
        assert_eq!(first, second);
    }

    #[test]
    fn a_changed_name_is_published_over_the_old_one() {
        let dir = TempDir::new("authors-rename");
        publish(&dir.0, "author-1", "Jules", None, None);
        publish(&dir.0, "author-1", "Jules F", None, None);
        assert_eq!(read_all(&dir.0).get("author-1"), Some(&profile("Jules F")));
    }

    #[test]
    fn several_authors_live_side_by_side() {
        // Each person writes only their own folder, which is what keeps the
        // registry free of conflicts when two of them sync one project.
        let dir = TempDir::new("authors-several");
        publish(&dir.0, "author-1", "Jules", None, None);
        publish(&dir.0, "author-2", "Manu", None, None);
        let all = read_all(&dir.0);
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("author-2"), Some(&profile("Manu")));
    }

    #[test]
    fn a_picture_is_copied_in_beside_the_record() {
        // A collaborator can reach the project folder and nothing else of ours,
        // so a path into our settings would be a picture they never see.
        let dir = TempDir::new("authors-avatar");
        let source = dir.0.join("me.png");
        fs::write(&source, b"pixels").expect("should write");

        publish(&dir.0, "author-1", "Jules", Some(&source), None);

        let folder = dir.0.join(AUTHORS_DIR).join("author-1");
        assert_eq!(
            read_all(&dir.0).get("author-1").and_then(|p| p.avatar.clone()),
            Some("me.png".to_owned())
        );
        assert_eq!(fs::read(folder.join("me.png")).expect("should read"), b"pixels");
    }

    #[test]
    fn a_record_naming_a_picture_that_is_not_there_is_finished_off() {
        // The record and the picture are two writes, and a run that stopped in
        // between must not leave a name pointing at nothing for good.
        let dir = TempDir::new("authors-half");
        let source = dir.0.join("me.png");
        fs::write(&source, b"pixels").expect("should write");
        publish(&dir.0, "author-1", "Jules", Some(&source), None);

        let copied = dir.0.join(AUTHORS_DIR).join("author-1").join("me.png");
        fs::remove_file(&copied).expect("should remove");

        publish(&dir.0, "author-1", "Jules", Some(&source), None);
        assert!(copied.is_file());
    }

    #[test]
    fn a_second_device_finds_the_author_it_already_is() {
        // Nothing carries an author id between machines, so this is the only
        // way a phone and a laptop end up one author rather than two.
        let dir = TempDir::new("authors-account");
        publish(&dir.0, "author-1", "Jules", None, Some("google:12345"));

        assert_eq!(
            id_for_account(&dir.0, "google:12345").as_deref(),
            Some("author-1")
        );
        assert_eq!(id_for_account(&dir.0, "google:somebody-else"), None);
    }

    #[test]
    fn signing_in_with_a_second_account_does_not_forget_the_first() {
        let dir = TempDir::new("authors-two-accounts");
        publish(&dir.0, "author-1", "Jules", None, Some("google:12345"));
        publish(&dir.0, "author-1", "Jules", None, Some("dropbox:67890"));

        let all = read_all(&dir.0);
        let profile = all.get("author-1").expect("should be there");
        assert_eq!(profile.accounts, ["google:12345", "dropbox:67890"]);
        // And publishing the same one again does not list it twice.
        publish(&dir.0, "author-1", "Jules", None, Some("google:12345"));
        assert_eq!(read_all(&dir.0)["author-1"].accounts.len(), 2);
    }

    #[test]
    fn a_name_shown_only_here_survives_the_global_name_changing() {
        // The Discord model: the override is a decision made in this project,
        // and republishing the global name must not quietly undo it.
        let dir = TempDir::new("authors-display");
        publish(&dir.0, "author-1", "Jules", None, None);
        set_display_name(&dir.0, "author-1", Some("Jules Fouchy")).expect("should set");

        publish(&dir.0, "author-1", "jf", None, None);

        let all = read_all(&dir.0);
        let profile = all.get("author-1").expect("should be there");
        assert_eq!(profile.name, "jf", "the global name still follows");
        assert_eq!(profile.shown_name(), "Jules Fouchy", "the override wins here");
    }

    #[test]
    fn clearing_the_override_hands_the_name_back() {
        let dir = TempDir::new("authors-display-clear");
        publish(&dir.0, "author-1", "Jules", None, None);
        set_display_name(&dir.0, "author-1", Some("Someone Else")).expect("should set");
        set_display_name(&dir.0, "author-1", None).expect("should clear");
        assert_eq!(read_all(&dir.0)["author-1"].shown_name(), "Jules");
        // A name of nothing but spaces is no name at all.
        set_display_name(&dir.0, "author-1", Some("   ")).expect("should clear");
        assert_eq!(read_all(&dir.0)["author-1"].shown_name(), "Jules");
    }

    #[test]
    fn a_name_is_never_empty() {
        assert!(!name_from_the_machine().is_empty());
    }
}
