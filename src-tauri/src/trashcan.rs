//! A trash folder inside the project, instead of the system one.
//!
//! Deleting used to hand the file to the OS Recycle Bin, which is out of the
//! project. That was wrong in three ways at once, and this fixes all three:
//!
//! - **macOS could not undo.** `trash::os_limited`, the half of the crate that
//!   reads the bin back, does not exist there, because macOS offers no API for
//!   it. A rename inside the project behaves identically on all three
//!   platforms.
//! - **A shared project lost the file for everyone.** A delete that leaves the
//!   folder is a delete nobody else can take back, and the undo only worked on
//!   the machine that did it. Moving into the project makes the deletion a
//!   change like any other: it syncs, and it can be undone from any device.
//! - **The move could fail.** The shell refuses a folder another process holds
//!   open, which is why the old path retried for two seconds. A rename within
//!   one directory does not care.
//!
//! What it costs is that the trash is now the app's to empty, which the system
//! one did for us. See [`purge_expired`].
//!
//! ```text
//! <project>/.lazuli-trash/
//!   <deletion-uuid>/
//!     deleted.yaml     what was deleted, when, and by whom
//!     IMG_4821.jpg     the payload, under the name it had
//! ```

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic;
use crate::paths::unique_path;

/// The trash folder's name, inside a project or inside the projects directory.
pub const TRASH_DIR: &str = ".lazuli-trash";

/// The marker file naming what a deletion folder holds.
const MARKER_FILE: &str = "deleted.yaml";

/// How long a deleted thing keeps its contents.
///
/// Thirty days rather than a day or two, for two separate reasons. A journal is
/// not opened every day, so "I realise a week later" is the case this exists
/// for. And a device that has not synced since before the deletion still has
/// its own live copy; until it has seen the marker, purging early would let it
/// treat that copy as a new addition and put the entry back.
pub const RETENTION_DAYS: i64 = 30;

/// What a deletion folder records about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Where the payload came from, relative to the project root, so a restore
    /// knows where to put it back. Forward slashes, so a deletion made on
    /// Windows restores on a Mac.
    pub path: String,
    /// When it was deleted. RFC 3339 with the offset present, like an entry's
    /// `created` — see [`crate::dates`].
    pub deleted: DateTime<FixedOffset>,
    /// Which author deleted it, once entries carry authors. `None` for a
    /// deletion made before that, and for a project that has never been shared.
    #[serde(default)]
    pub by: Option<String>,
    /// Set once the payload has been handed on to the system Recycle Bin and
    /// only this marker is left.
    ///
    /// The marker outlives the payload on purpose: it is a few dozen bytes, and
    /// it is what tells a device syncing late that the thing was deleted rather
    /// than never seen. Dropping it is what would resurrect the entry.
    #[serde(default)]
    pub purged: bool,
    /// Fields this build does not know, carried through a rewrite untouched.
    ///
    /// A project folder is written by whichever build each person has, and a
    /// build that dropped what it did not understand would strip a newer
    /// build's fields every time it saved. This one is rewritten when its payload is purged.
    #[serde(flatten)]
    pub rest: BTreeMap<String, serde_yaml::Value>,
}

/// One thing in the trash, as the UI lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrashedItem {
    /// The deletion folder's name, and the handle for restoring or purging it.
    pub id: String,
    pub marker: Marker,
    /// False once the payload has been purged, when only the record remains.
    pub restorable: bool,
    /// What it is, in terms a person can recognise.
    pub what: What,
}

/// What a deletion holds, named the way the user would name it.
///
/// An entry's folder is a UUID, which says nothing to anyone, so an entry is
/// described by the day and the sentence it held instead. The date travels as
/// the day itself rather than as text: the UI decides whether a project reads
/// in dates or in day numbers, and the trash should agree with the timeline it
/// was deleted from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum What {
    Entry {
        date: NaiveDate,
        text: String,
    },
    /// An image, or a whole project folder.
    File {
        name: String,
    },
}

/// How much of an entry's sentence the trash shows before cutting it.
const LABEL_CHARS: usize = 60;

/// Describe a deletion's payload, falling back to its filename.
fn describe(payload: &Path) -> What {
    let name = payload
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Something")
        .to_owned();

    let entry_file = payload.join(crate::store::ENTRY_FILE);
    let Ok((frontmatter, text)) = crate::store::read_entry_file(&entry_file) else {
        return What::File { name };
    };

    let mut text: String = text.chars().take(LABEL_CHARS).collect();
    if text.chars().count() == LABEL_CHARS {
        text.push('…');
    }
    What::Entry {
        date: frontmatter.journal_date(),
        text,
    }
}

/// Move `path` into `root`'s trash, returning the deletion's id.
///
/// A path inside `root` — an entry or an image inside its project — records
/// where it was relative to it, so the project folder can be moved or synced to
/// another machine and the deletion still knows where to go back to. A path
/// outside, which is how a whole project is deleted into the projects
/// directory's trash, records its absolute path instead, there being nothing
/// for it to be relative to.
pub fn put(root: &Path, path: &Path, by: Option<&str>) -> Result<String> {
    let original = match path.strip_prefix(root) {
        Ok(relative) => relative
            .to_str()
            .with_context(|| format!("{} is not valid UTF-8", relative.display()))?
            .replace('\\', "/"),
        Err(_) => path
            .to_str()
            .with_context(|| format!("{} is not valid UTF-8", path.display()))?
            .replace('\\', "/"),
    };

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("{} has no filename", path.display()))?;

    let id = uuid::Uuid::new_v4().to_string();
    let folder = root.join(TRASH_DIR).join(&id);
    fs::create_dir_all(&folder)
        .with_context(|| format!("creating {}", folder.display()))?;

    // The move is the delete. Everything after it is bookkeeping, so it goes
    // first: a marker with no payload would be a deletion that cannot be undone.
    move_path(path, &folder.join(name))?;

    let marker = Marker {
        path: original,
        deleted: Utc::now().into(),
        by: by.map(str::to_owned),
        purged: false,
        rest: BTreeMap::new(),
    };
    write_marker(&folder, &marker)?;
    Ok(id)
}

/// Put deletion `id` back where it came from, returning the name it landed
/// under.
///
/// Its own name is taken when something has been put there since, in which case
/// the restored file takes the ` (2)` suffix rather than the occupant: the
/// occupant is the file the user has been looking at and may already be a
/// chosen image, and it should not change name under them.
pub fn restore(root: &Path, id: &str) -> Result<String> {
    let folder = root.join(TRASH_DIR).join(id);
    let marker = read_marker(&folder)?;
    if marker.purged {
        bail!(
            "{} was emptied from the trash after {RETENTION_DAYS} days, \
             and is in the system Recycle Bin instead.",
            marker.path
        );
    }

    let payload = payload_of(&folder)?;
    let recorded = PathBuf::from(marker.path.replace('/', std::path::MAIN_SEPARATOR_STR));
    // A whole project recorded where it was outright; everything inside a
    // project recorded where it was within one, so that the folder can have
    // moved since.
    let original = if recorded.is_absolute() {
        recorded
    } else {
        root.join(recorded)
    };
    let parent = original
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent folder", original.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating {}", parent.display()))?;

    let name = original
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("{} has no filename", original.display()))?;
    let destination = unique_path(parent, name);

    move_path(&payload, &destination)
        .with_context(|| format!("restoring {}", destination.display()))?;

    // The record has served its purpose; leaving it would tell the next device
    // to sync that this was deleted, and undo the undo.
    fs::remove_dir_all(&folder)
        .with_context(|| format!("clearing {}", folder.display()))?;

    Ok(destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(name)
        .to_owned())
}

/// Everything in `root`'s trash, most recently deleted first.
pub fn list(root: &Path) -> Vec<TrashedItem> {
    let Ok(entries) = fs::read_dir(root.join(TRASH_DIR)) else {
        return Vec::new();
    };
    let mut items: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_owned();
            let marker = read_marker(&entry.path()).ok()?;
            let restorable = !marker.purged;
            // A purged deletion has no payload left to describe, so its own
            // record is all there is to go on.
            let what = payload_of(&entry.path()).map_or_else(
                |_| What::File {
                    name: marker
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&marker.path)
                        .to_owned(),
                },
                |payload| describe(&payload),
            );
            Some(TrashedItem {
                id,
                marker,
                restorable,
                what,
            })
        })
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.marker.deleted));
    items
}

/// Hand the payload of anything older than [`RETENTION_DAYS`] to the system
/// Recycle Bin, keeping its marker.
///
/// This is the one place the app removes something the user did not just ask it
/// to remove, and it is why the payload goes to the system bin rather than
/// being unlinked: the promise is that Lazuli never destroys anything, and the
/// in-project trash is a grace period in front of the system one rather than a
/// replacement for it.
///
/// Returns how many deletions were purged. Failures are skipped rather than
/// propagated: this runs in the background, and a folder something holds open
/// is a reason to try again next time, not to report an error over a deletion
/// the user finished with a month ago.
pub fn purge_expired(root: &Path) -> usize {
    let cutoff = Utc::now() - chrono::Duration::days(RETENTION_DAYS);
    let mut purged = 0;
    for item in list(root) {
        if item.marker.purged || item.marker.deleted >= cutoff {
            continue;
        }
        let folder = root.join(TRASH_DIR).join(&item.id);
        let Ok(payload) = payload_of(&folder) else {
            continue;
        };
        if trash::delete(&payload).is_err() {
            continue;
        }
        let marker = Marker {
            purged: true,
            ..item.marker
        };
        if write_marker(&folder, &marker).is_ok() {
            purged += 1;
        }
    }
    purged
}

/// Move a file or folder, falling back to a copy when a rename cannot cross
/// what is between the two paths.
///
/// A rename is atomic, instant whatever the size, and the reason deleting no
/// longer has to fight anything holding the file open. It only works within one
/// filesystem, which covers everything inside a project and the usual case of a
/// project inside the projects directory. A project kept on another drive is
/// the exception, and there the bytes genuinely have to be copied.
fn move_path(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Ok(()) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(err).with_context(|| format!("moving {}", from.display()))
        }
        // Anything else may be a different filesystem, which `ErrorKind` has no
        // portable name for: Windows reports `ERROR_NOT_SAME_DEVICE` and Unix
        // `EXDEV`, and neither maps to a stable variant. Try the copy and let
        // *that* report the real problem if there is one.
        Err(_) => {}
    }

    copy_recursively(from, to)
        .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;

    // Only once the copy is complete, so a failure halfway leaves the original
    // where it was rather than losing it between two places.
    if from.is_dir() {
        fs::remove_dir_all(from)
    } else {
        fs::remove_file(from)
    }
    .with_context(|| format!("removing {} after copying it", from.display()))
}

fn copy_recursively(from: &Path, to: &Path) -> Result<()> {
    if from.is_file() {
        fs::copy(from, to)?;
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        copy_recursively(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

/// The payload inside a deletion folder: the one thing in it that is not the
/// marker.
fn payload_of(folder: &Path) -> Result<PathBuf> {
    fs::read_dir(folder)
        .with_context(|| format!("reading {}", folder.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.file_name().is_some_and(|name| name != MARKER_FILE))
        .ok_or_else(|| anyhow!("{} holds no deleted file", folder.display()))
}

fn write_marker(folder: &Path, marker: &Marker) -> Result<()> {
    let yaml = serde_yaml::to_string(marker).context("serialising a deletion record")?;
    atomic::write(&folder.join(MARKER_FILE), yaml)
}

fn read_marker(folder: &Path) -> Result<Marker> {
    let path = folder.join(MARKER_FILE);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml::from_str(&contents).with_context(|| format!("parsing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_a_newer_build_wrote_survives_the_marker_being_rewritten() {
        // A marker is rewritten when its payload is purged, by whichever build
        // happens to open the project thirty days later.
        let folder = std::env::temp_dir().join(format!("lazuli-marker-rest-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&folder).expect("should create");
        fs::write(
            folder.join(MARKER_FILE),
            "path: entries/e\ndeleted: 2026-06-10T09:00:00+02:00\nby: null\npurged: false\nreason: tidying\n",
        )
        .expect("should write");

        let marker = read_marker(&folder).expect("should read");
        write_marker(&folder, &Marker { purged: true, ..marker }).expect("should write");

        let after = fs::read_to_string(folder.join(MARKER_FILE)).expect("should read");
        assert!(after.contains("purged: true"));
        assert!(after.contains("reason: tidying"), "{after}");
        let _ = fs::remove_dir_all(&folder);
    }

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

    /// A project root with `entries/<id>/photo.jpg` in it.
    fn project(label: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new(label);
        let entry = dir.0.join("entries").join("an-entry");
        fs::create_dir_all(&entry).expect("should create the entry folder");
        fs::write(entry.join("photo.jpg"), b"pixels").expect("should write the image");
        (dir, entry)
    }

    #[test]
    fn a_file_moves_into_the_trash_and_comes_back() {
        let (dir, entry) = project("trash-round-trip");
        let image = entry.join("photo.jpg");

        let id = put(&dir.0, &image, None).expect("should trash");
        assert!(!image.exists(), "the image should have left its folder");

        let name = restore(&dir.0, &id).expect("should restore");
        assert_eq!(name, "photo.jpg");
        assert_eq!(
            fs::read(&image).expect("should read the restored image"),
            b"pixels"
        );
    }

    #[test]
    fn a_whole_entry_folder_moves_and_comes_back() {
        let (dir, entry) = project("trash-entry");
        let id = put(&dir.0, &entry, None).expect("should trash");
        assert!(!entry.exists());

        restore(&dir.0, &id).expect("should restore");
        assert_eq!(
            fs::read(entry.join("photo.jpg")).expect("should read"),
            b"pixels"
        );
    }

    #[test]
    fn the_record_is_gone_once_it_has_been_restored() {
        // Left behind, it would tell the next device to sync that the entry was
        // deleted, and undo the undo.
        let (dir, entry) = project("trash-record");
        let id = put(&dir.0, &entry, None).expect("should trash");
        restore(&dir.0, &id).expect("should restore");
        assert!(list(&dir.0).is_empty());
    }

    #[test]
    fn a_name_taken_since_the_delete_sends_the_restored_file_to_a_suffix() {
        // The occupant is what the user has been looking at and may be a chosen
        // image; it must not change name under them.
        let (dir, entry) = project("trash-collision");
        let image = entry.join("photo.jpg");
        let id = put(&dir.0, &image, None).expect("should trash");
        fs::write(&image, b"a different picture").expect("should write");

        let name = restore(&dir.0, &id).expect("should restore");
        assert_eq!(name, "photo (2).jpg");
        assert_eq!(
            fs::read(&image).expect("should read"),
            b"a different picture"
        );
        assert_eq!(
            fs::read(entry.join("photo (2).jpg")).expect("should read"),
            b"pixels"
        );
    }

    #[test]
    fn a_deletion_records_where_it_came_from_with_forward_slashes() {
        // A deletion made on Windows has to restore on a Mac.
        let (dir, entry) = project("trash-path");
        put(&dir.0, &entry.join("photo.jpg"), None).expect("should trash");
        let items = list(&dir.0);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].marker.path, "entries/an-entry/photo.jpg");
        assert!(items[0].restorable);
    }

    #[test]
    fn the_author_is_recorded_when_there_is_one() {
        let (dir, entry) = project("trash-author");
        put(&dir.0, &entry.join("photo.jpg"), Some("author-uuid")).expect("should trash");
        assert_eq!(
            list(&dir.0)[0].marker.by.as_deref(),
            Some("author-uuid")
        );
    }

    #[test]
    fn nothing_recent_is_purged() {
        let (dir, entry) = project("trash-keep");
        put(&dir.0, &entry.join("photo.jpg"), None).expect("should trash");
        assert_eq!(purge_expired(&dir.0), 0);
        assert!(list(&dir.0)[0].restorable);
    }

    #[test]
    fn a_purged_deletion_keeps_its_marker_and_refuses_to_restore() {
        // The marker outliving the payload is the whole point: it is what stops
        // a device that synced late putting the entry back.
        let (dir, entry) = project("trash-purge");
        let id = put(&dir.0, &entry.join("photo.jpg"), None).expect("should trash");

        let folder = dir.0.join(TRASH_DIR).join(&id);
        let mut marker = read_marker(&folder).expect("should read");
        marker.deleted = (Utc::now() - chrono::Duration::days(RETENTION_DAYS + 1)).into();
        write_marker(&folder, &marker).expect("should write");

        assert_eq!(purge_expired(&dir.0), 1);

        let items = list(&dir.0);
        assert_eq!(items.len(), 1, "the record must outlive the payload");
        assert!(!items[0].restorable);
        assert!(restore(&dir.0, &id).is_err());
    }

    #[test]
    fn something_from_outside_records_where_it_was_outright() {
        // How a whole project is deleted: it cannot go inside its own trash, so
        // it goes into the projects directory's, with nothing to be relative to.
        let dir = TempDir::new("trash-foreign");
        let trash_root = dir.0.join("projects");
        let project = dir.0.join("elsewhere").join("A Journal");
        fs::create_dir_all(&project).expect("should create");
        fs::write(project.join("lazuli.yaml"), b"name: A Journal").expect("should write");
        fs::create_dir_all(&trash_root).expect("should create");

        let id = put(&trash_root, &project, None).expect("should trash");
        assert!(!project.exists());
        assert!(PathBuf::from(&list(&trash_root)[0].marker.path).is_absolute());

        restore(&trash_root, &id).expect("should restore");
        assert_eq!(
            fs::read(project.join("lazuli.yaml")).expect("should read"),
            b"name: A Journal"
        );
    }

    #[test]
    fn a_deletion_survives_the_project_folder_being_moved() {
        // The whole reason a path inside a project is recorded relative to it:
        // the folder syncs to another machine, under another path, and a
        // deletion made before the move still knows where to go back to.
        let (dir, entry) = project("trash-moved");
        let id = put(&dir.0, &entry.join("photo.jpg"), None).expect("should trash");

        let moved = dir.0.with_file_name(format!(
            "{}-moved",
            dir.0.file_name().expect("has a name").to_string_lossy()
        ));
        fs::rename(&dir.0, &moved).expect("should move the project");

        restore(&moved, &id).expect("should restore at the new path");
        assert!(moved.join("entries/an-entry/photo.jpg").exists());
        let _ = fs::rename(&moved, &dir.0);
    }

    #[test]
    fn a_deleted_entry_is_described_by_its_day_and_its_sentence() {
        // Its folder is a UUID, which tells the user nothing about what they
        // are being offered back.
        let dir = TempDir::new("trash-label");
        let entry = dir.0.join("entries").join("aaaa-bbbb");
        fs::create_dir_all(&entry).expect("should create");
        fs::write(
            entry.join("entry.md"),
            "---\ndate: 2026-09-20\ncreated: 2026-09-20T10:00:00+02:00\nimage: null\n---\n\nThe day the roof went on.\n",
        )
        .expect("should write");

        put(&dir.0, &entry, None).expect("should trash");
        match &list(&dir.0)[0].what {
            What::Entry { date, text } => {
                assert_eq!(date.to_string(), "2026-09-20");
                assert_eq!(text, "The day the roof went on.");
            }
            other => panic!("expected an entry, got {other:?}"),
        }
    }

    #[test]
    fn a_deleted_image_is_described_by_its_filename() {
        let (dir, entry) = project("trash-label-image");
        put(&dir.0, &entry.join("photo.jpg"), None).expect("should trash");
        assert_eq!(
            list(&dir.0)[0].what,
            What::File {
                name: "photo.jpg".into()
            }
        );
    }

    #[test]
    fn an_empty_trash_lists_nothing() {
        let dir = TempDir::new("trash-empty");
        assert!(list(&dir.0).is_empty());
    }
}
