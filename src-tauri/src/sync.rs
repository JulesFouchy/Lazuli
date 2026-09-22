//! Reconciling a project folder with a copy of it somewhere else.
//!
//! The engine knows nothing about *where* the other copy is: a [`Backend`] is
//! four operations over a dumb store of files, and Google Drive, Dropbox, a
//! folder on another disk and the in-memory one the tests use all fit it. What
//! it does know is how to decide, for each file, which side is right.
//!
//! **It writes files and stops there.** The watcher picks the writes up and the
//! existing rescan-and-diff puts them on screen, so nothing here touches the
//! app's state and nothing in the app has to know a sync happened. That is the
//! whole of its integration with the rest of the program.
//!
//! # Deciding
//!
//! Three things are known about each path: what is on disk now, what the remote
//! has now, and **what the two last agreed on** — the base, kept per device in
//! `.lazuli/sync-state.json`. The base is what makes this a decision rather
//! than a guess: without it, "they changed it" and "I changed it" look the same
//! from here.
//!
//! # What conflicts, and what cannot
//!
//! Very little, because of how a project is laid out:
//!
//! - Entry folders are UUIDs, so two devices adding entries never collide.
//! - Images are written once and never edited, so an image can only conflict by
//!   two devices independently adding the same *name*.
//! - Each author writes only their own record.
//!
//! What is left is one entry's sentence edited in two places, and `lazuli.yaml`.
//! Neither is merged: the remote's copy is written down beside the local one as
//! `<name>.sync-conflict-<stamp><ext>`, which for an `entry.md` is exactly what
//! [`crate::conflicts`] already draws a card for, and for an image is another
//! candidate in the entry's folder — the "keep both and ask" rule `wip.md` set
//! out, falling out of the layout rather than being implemented twice.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic;

/// The per-device folder: what this machine knows, which no other machine wants.
pub const STATE_DIR: &str = ".lazuli";

const STATE_FILE: &str = "sync-state.json";

/// A file as the remote has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    /// Relative to the project, with forward slashes, whatever the platform.
    pub path: String,
    /// Opaque, and only ever compared: whatever the backend changes when the
    /// contents change. Drive's version, an ETag, a hash — the engine does not
    /// care which.
    pub revision: String,
}

/// A dumb store of files, which is all a remote has to be.
///
/// Deliberately four operations and no more. Anything that cannot be expressed
/// in them — a merge, a history, a lock — is the engine's job or nobody's, and
/// that is what keeps a second backend cheap to add.
pub trait Backend {
    fn list(&self) -> Result<Vec<RemoteFile>>;
    fn get(&self, path: &str) -> Result<Vec<u8>>;
    /// Write `bytes`, returning the revision they landed under.
    ///
    /// `expected` is the revision this write believes it is replacing, for a
    /// backend that can refuse a write that would overwrite a newer one. A
    /// backend without that ability ignores it; the cost is a window between
    /// listing and writing in which a change can be missed, which the next pass
    /// then sees as an ordinary conflict.
    fn put(&self, path: &str, bytes: &[u8], expected: Option<&str>) -> Result<String>;
    fn delete(&self, path: &str) -> Result<()>;
}

/// What the two sides last agreed on, for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agreed {
    /// The content hash the local file had.
    pub hash: String,
    /// The revision the remote reported.
    pub revision: String,
}

/// The base, as kept on this device.
///
/// A `BTreeMap` so the file is in a stable order and reads as a diff rather
/// than as a reshuffle. Per device and **never synced**: it is this machine's
/// account of what it has seen, and another machine's would be a lie here.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Which remote this base describes.
    ///
    /// Load-bearing, and learned the hard way: a base is a record of a
    /// conversation with *one* remote. Applied to a different one it reads every
    /// file the new remote has not got — which is all of them — as something the
    /// other side deleted, and deletes it locally. Kept here so that a base
    /// belonging to another remote can be spotted and dropped instead.
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub files: BTreeMap<String, Agreed>,
}

impl State {
    /// The base, but only if it is the base for `folder`.
    ///
    /// A base from another remote is discarded rather than trusted: with no
    /// base the two sides are compared from scratch, which at worst costs a
    /// download and a few conflicts, where the wrong base costs files.
    pub fn read(root: &Path, folder: &str) -> Self {
        let stored: Self = fs::read_to_string(root.join(STATE_DIR).join(STATE_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if stored.folder == folder {
            return stored;
        }
        Self {
            folder: folder.to_owned(),
            files: BTreeMap::new(),
        }
    }

    pub fn write(&self, root: &Path) -> Result<()> {
        let directory = root.join(STATE_DIR);
        fs::create_dir_all(&directory)
            .with_context(|| format!("creating {}", directory.display()))?;
        let text = serde_json::to_string_pretty(self).context("serialising the sync state")?;
        atomic::write(&directory.join(STATE_FILE), text)
    }
}

/// What to do about one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Ours is newer, or ours is new.
    Upload(String),
    /// Theirs is newer, or theirs is new.
    Download(String),
    /// We deleted it and they have not touched it since.
    DeleteRemote(String),
    /// They deleted it and we have not touched it since.
    DeleteLocal(String),
    /// Both sides changed it. The bytes still have to be compared before this
    /// is believed — see [`Outcome`].
    Conflict(String),
    /// Gone from both sides; only the base still mentions it.
    Forget(String),
}

impl Action {
    pub fn path(&self) -> &str {
        match self {
            Action::Upload(path)
            | Action::Download(path)
            | Action::DeleteRemote(path)
            | Action::DeleteLocal(path)
            | Action::Conflict(path)
            | Action::Forget(path) => path,
        }
    }
}

/// Decide what to do about every path either side knows about.
///
/// Pure, and the whole of the engine's judgement: everything else is carrying
/// bytes. `local` maps a path to its content hash, `remote` to its revision.
pub fn plan(
    local: &BTreeMap<String, String>,
    remote: &BTreeMap<String, String>,
    base: &State,
) -> Vec<Action> {
    let paths: BTreeSet<&String> = local
        .keys()
        .chain(remote.keys())
        .chain(base.files.keys())
        .collect();

    let mut actions = Vec::new();
    for path in paths {
        let here = local.get(path);
        let there = remote.get(path);
        let agreed = base.files.get(path);

        let action = match (here, there, agreed) {
            // Both have it, and both know what it was.
            (Some(hash), Some(revision), Some(agreed)) => {
                match (hash != &agreed.hash, revision != &agreed.revision) {
                    (false, false) => continue,
                    (true, false) => Action::Upload(path.clone()),
                    (false, true) => Action::Download(path.clone()),
                    (true, true) => Action::Conflict(path.clone()),
                }
            }
            // Both have it and neither has seen the other's: two devices made
            // the same path independently.
            (Some(_), Some(_), None) => Action::Conflict(path.clone()),

            // Gone from the remote. If we have not touched it since we agreed,
            // they deleted it and we follow. If we have, our edit outlives
            // their delete — the deletion is in their `.lazuli-trash/` and can
            // be had back, where an unsaved edit cannot.
            (Some(hash), None, Some(agreed)) => {
                if hash == &agreed.hash {
                    Action::DeleteLocal(path.clone())
                } else {
                    Action::Upload(path.clone())
                }
            }
            (Some(_), None, None) => Action::Upload(path.clone()),

            // Gone from here, by the same rule the other way round.
            (None, Some(revision), Some(agreed)) => {
                if revision == &agreed.revision {
                    Action::DeleteRemote(path.clone())
                } else {
                    Action::Download(path.clone())
                }
            }
            (None, Some(_), None) => Action::Download(path.clone()),

            (None, None, Some(_)) => Action::Forget(path.clone()),
            (None, None, None) => continue,
        };
        actions.push(action);
    }
    actions
}

/// What a pass did, for the status line and for the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub uploaded: usize,
    pub downloaded: usize,
    pub deleted_here: usize,
    pub deleted_there: usize,
    /// Both sides changed and the bytes really differ; the remote's copy is now
    /// beside ours for the user to choose from.
    pub conflicted: usize,
    /// Files this pass could not carry, and what stopped the last of them.
    ///
    /// One file is not the pass: a photograph that timed out must not hold up
    /// the sentence in the entry beside it, and the next pass will try it again
    /// because the base still says it has not been agreed.
    pub failed: usize,
    pub problem: Option<String>,
}

impl Outcome {
    pub fn did_nothing(&self) -> bool {
        *self == Outcome::default()
    }
}

/// Bring the project and the remote into agreement, once.
pub fn run(root: &Path, backend: &dyn Backend) -> Result<Outcome> {
    let mut state = State::read(root, &Config::read(root).folder);
    let local = scan_local(root)?;
    let remote: BTreeMap<String, String> = backend
        .list()
        .context("listing the remote")?
        .into_iter()
        .map(|file| (file.path, file.revision))
        .collect();

    let mut outcome = Outcome::default();
    for action in plan(&local, &remote, &state) {
        if let Err(err) = apply(root, backend, &action, &mut state, &mut outcome) {
            // Carried on from rather than given up at. The base is untouched
            // for this path, so the next pass sees the same work to do and
            // tries again — which is what makes a dropped connection cost a
            // minute rather than a sync.
            outcome.failed += 1;
            outcome.problem = Some(format!("{}: {err:#}", action.path()));
            continue;
        }
        // Written after each file rather than at the end: a pass that stops
        // halfway — the network drops, the app is closed — must not have to
        // start over, and a base that is behind costs a comparison where a base
        // that is ahead would lose a change.
        state.write(root)?;
    }
    Ok(outcome)
}

fn apply(
    root: &Path,
    backend: &dyn Backend,
    action: &Action,
    state: &mut State,
    outcome: &mut Outcome,
) -> Result<()> {
    let path = action.path();
    match action {
        Action::Upload(_) => {
            let bytes = fs::read(local_path(root, path))?;
            let expected = state.files.get(path).map(|agreed| agreed.revision.clone());
            let revision = backend.put(path, &bytes, expected.as_deref())?;
            state.files.insert(
                path.to_owned(),
                Agreed {
                    hash: hash(&bytes),
                    revision,
                },
            );
            outcome.uploaded += 1;
        }
        Action::Download(_) => {
            let bytes = backend.get(path)?;
            write_local(root, path, &bytes)?;
            let revision = remote_revision(backend, path)?;
            state.files.insert(
                path.to_owned(),
                Agreed {
                    hash: hash(&bytes),
                    revision,
                },
            );
            outcome.downloaded += 1;
        }
        Action::DeleteRemote(_) => {
            backend.delete(path)?;
            state.files.remove(path);
            outcome.deleted_there += 1;
        }
        Action::DeleteLocal(_) => {
            let file = local_path(root, path);
            if file.exists() {
                fs::remove_file(&file)
                    .with_context(|| format!("removing {}", file.display()))?;
            }
            state.files.remove(path);
            outcome.deleted_here += 1;
        }
        Action::Conflict(_) => {
            let theirs = backend.get(path)?;
            let ours = fs::read(local_path(root, path))?;
            if ours == theirs {
                // Both sides did the same thing, which is not a disagreement.
                state.files.insert(
                    path.to_owned(),
                    Agreed {
                        hash: hash(&ours),
                        revision: remote_revision(backend, path)?,
                    },
                );
                return Ok(());
            }
            // Theirs lands beside ours under the name a folder syncer would
            // have used, which is the one the conflict card already knows.
            let beside = conflict_path(path);
            write_local(root, &beside, &theirs)?;
            // Ours stays as it is and stays the one the remote holds, so the
            // other device is not asked the same question twice.
            let revision = backend.put(path, &ours, None)?;
            state.files.insert(
                path.to_owned(),
                Agreed {
                    hash: hash(&ours),
                    revision,
                },
            );
            outcome.conflicted += 1;
        }
        Action::Forget(_) => {
            state.files.remove(path);
        }
    }
    Ok(())
}

/// The revision a path is at now, for a backend whose `put` did not say.
fn remote_revision(backend: &dyn Backend, path: &str) -> Result<String> {
    Ok(backend
        .list()?
        .into_iter()
        .find(|file| file.path == path)
        .map(|file| file.revision)
        .unwrap_or_default())
}

/// `entries/x/entry.md` → `entries/x/entry.sync-conflict-<stamp>.md`.
///
/// Deliberately the shape Syncthing uses, because [`crate::conflicts`] already
/// recognises it and an image that lands this way is simply another candidate
/// in the entry's folder.
fn conflict_path(path: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    match path.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => {
            format!("{stem}.sync-conflict-{stamp}.{extension}")
        }
        _ => format!("{path}.sync-conflict-{stamp}"),
    }
}

fn local_path(root: &Path, path: &str) -> PathBuf {
    root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn write_local(root: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let file = local_path(root, path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    atomic::write(&file, bytes)
}

/// Every file in the project that takes part, by path and content hash.
pub fn scan_local(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    walk(root, root, &mut found)?;
    Ok(found)
}

fn walk(root: &Path, directory: &Path, into: &mut BTreeMap<String, String>) -> Result<()> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Some(relative) = relative_of(root, &path) else {
            continue;
        };
        if !is_synced(&relative) {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, into)?;
        } else if let Ok(bytes) = fs::read(&path) {
            into.insert(relative, hash(&bytes));
        }
    }
    Ok(())
}

fn relative_of(root: &Path, path: &Path) -> Option<String> {
    Some(
        path.strip_prefix(root)
            .ok()?
            .to_str()?
            .replace('\\', "/"),
    )
}

/// Whether a path travels with the project.
///
/// Everything does except this device's own account of the sync. The trash
/// travels *on purpose*: a deletion is a change like any other, and it is the
/// only way the other machine learns about one.
pub fn is_synced(relative: &str) -> bool {
    let first = relative.split('/').next().unwrap_or(relative);
    first != STATE_DIR
}

/// The content hash the base is kept in terms of.
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A remote that lives in memory, which is the whole point of the trait.
    #[derive(Default)]
    struct Memory {
        files: RefCell<HashMap<String, (Vec<u8>, u64)>>,
        clock: RefCell<u64>,
    }

    impl Memory {
        fn bump(&self) -> u64 {
            let mut clock = self.clock.borrow_mut();
            *clock += 1;
            *clock
        }
    }

    impl Backend for Memory {
        fn list(&self) -> Result<Vec<RemoteFile>> {
            Ok(self
                .files
                .borrow()
                .iter()
                .map(|(path, (_, revision))| RemoteFile {
                    path: path.clone(),
                    revision: revision.to_string(),
                })
                .collect())
        }

        fn get(&self, path: &str) -> Result<Vec<u8>> {
            self.files
                .borrow()
                .get(path)
                .map(|(bytes, _)| bytes.clone())
                .ok_or_else(|| anyhow::anyhow!("{path} is not on the remote"))
        }

        fn put(&self, path: &str, bytes: &[u8], _expected: Option<&str>) -> Result<String> {
            let revision = self.bump();
            self.files
                .borrow_mut()
                .insert(path.to_owned(), (bytes.to_vec(), revision));
            Ok(revision.to_string())
        }

        fn delete(&self, path: &str) -> Result<()> {
            self.files.borrow_mut().remove(path);
            Ok(())
        }
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

    fn write(root: &Path, path: &str, contents: &str) {
        let file = local_path(root, path);
        fs::create_dir_all(file.parent().expect("has a parent")).expect("should create");
        fs::write(file, contents).expect("should write");
    }

    fn read(root: &Path, path: &str) -> String {
        fs::read_to_string(local_path(root, path)).expect("should read")
    }

    fn base_of(pairs: &[(&str, &str, &str)]) -> State {
        State {
            folder: String::new(),
            files: pairs
                .iter()
                .map(|(path, hash, revision)| {
                    (
                        (*path).to_owned(),
                        Agreed {
                            hash: (*hash).to_owned(),
                            revision: (*revision).to_owned(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    // --- the decision table ------------------------------------------------

    #[test]
    fn a_file_neither_side_touched_is_left_alone() {
        let actions = plan(
            &map(&[("a", "h1")]),
            &map(&[("a", "r1")]),
            &base_of(&[("a", "h1", "r1")]),
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn our_change_goes_up_and_theirs_comes_down() {
        assert_eq!(
            plan(
                &map(&[("a", "h2")]),
                &map(&[("a", "r1")]),
                &base_of(&[("a", "h1", "r1")])
            ),
            vec![Action::Upload("a".into())]
        );
        assert_eq!(
            plan(
                &map(&[("a", "h1")]),
                &map(&[("a", "r2")]),
                &base_of(&[("a", "h1", "r1")])
            ),
            vec![Action::Download("a".into())]
        );
    }

    #[test]
    fn a_file_new_on_one_side_goes_to_the_other() {
        assert_eq!(
            plan(&map(&[("a", "h1")]), &map(&[]), &State::default()),
            vec![Action::Upload("a".into())]
        );
        assert_eq!(
            plan(&map(&[]), &map(&[("a", "r1")]), &State::default()),
            vec![Action::Download("a".into())]
        );
    }

    #[test]
    fn a_delete_the_other_side_has_not_touched_is_followed() {
        assert_eq!(
            plan(&map(&[]), &map(&[("a", "r1")]), &base_of(&[("a", "h1", "r1")])),
            vec![Action::DeleteRemote("a".into())]
        );
        assert_eq!(
            plan(&map(&[("a", "h1")]), &map(&[]), &base_of(&[("a", "h1", "r1")])),
            vec![Action::DeleteLocal("a".into())]
        );
    }

    #[test]
    fn an_edit_outlives_the_other_sides_delete() {
        // A deletion is recoverable from the other machine's trash; an edit
        // that was thrown away is not recoverable from anywhere.
        assert_eq!(
            plan(&map(&[("a", "h2")]), &map(&[]), &base_of(&[("a", "h1", "r1")])),
            vec![Action::Upload("a".into())]
        );
        assert_eq!(
            plan(&map(&[]), &map(&[("a", "r2")]), &base_of(&[("a", "h1", "r1")])),
            vec![Action::Download("a".into())]
        );
    }

    #[test]
    fn both_sides_changing_it_is_a_conflict() {
        assert_eq!(
            plan(
                &map(&[("a", "h2")]),
                &map(&[("a", "r2")]),
                &base_of(&[("a", "h1", "r1")])
            ),
            vec![Action::Conflict("a".into())]
        );
        // And with no base at all: two devices made the same path.
        assert_eq!(
            plan(&map(&[("a", "h1")]), &map(&[("a", "r1")]), &State::default()),
            vec![Action::Conflict("a".into())]
        );
    }

    #[test]
    fn a_path_only_the_base_remembers_is_forgotten() {
        assert_eq!(
            plan(&map(&[]), &map(&[]), &base_of(&[("a", "h1", "r1")])),
            vec![Action::Forget("a".into())]
        );
    }

    // --- what takes part ---------------------------------------------------

    #[test]
    fn this_devices_own_account_of_the_sync_does_not_travel() {
        assert!(!is_synced(".lazuli/sync-state.json"));
        assert!(!is_synced(".lazuli/sync.json"));
    }

    #[test]
    fn everything_else_does_including_the_trash() {
        // A deletion is a change like any other, and the trash is the only way
        // the other machine learns about one.
        assert!(is_synced("lazuli.yaml"));
        assert!(is_synced("entries/an-id/entry.md"));
        assert!(is_synced("cover/banner.jpg"));
        assert!(is_synced("authors/an-id/profile.yaml"));
        assert!(is_synced(".lazuli-trash/a-deletion/deleted.yaml"));
        assert!(is_synced(".lazuli-thumbs/entries/an-id/photo.jpg"));
    }

    // --- carrying the bytes ------------------------------------------------

    #[test]
    fn a_fresh_project_goes_up_whole() {
        let dir = TempDir::new("sync-up");
        write(&dir.0, "lazuli.yaml", "name: P\n");
        write(&dir.0, "entries/one/entry.md", "first\n");
        let remote = Memory::default();

        let outcome = run(&dir.0, &remote).expect("should sync");

        assert_eq!(outcome.uploaded, 2);
        assert_eq!(remote.files.borrow().len(), 2);
        // And a second pass has nothing to do, which is what makes it safe to
        // run on a timer.
        assert!(run(&dir.0, &remote).expect("should sync").did_nothing());
    }

    #[test]
    fn an_empty_project_takes_the_whole_remote_down() {
        let dir = TempDir::new("sync-down");
        let remote = Memory::default();
        remote.put("lazuli.yaml", b"name: P\n", None).expect("put");
        remote.put("entries/one/entry.md", b"first\n", None).expect("put");

        let outcome = run(&dir.0, &remote).expect("should sync");

        assert_eq!(outcome.downloaded, 2);
        assert_eq!(read(&dir.0, "entries/one/entry.md"), "first\n");
    }

    /// A remote that refuses to write one particular path.
    struct Awkward {
        inner: Memory,
        refuse: String,
    }

    impl Backend for Awkward {
        fn list(&self) -> Result<Vec<RemoteFile>> {
            self.inner.list()
        }
        fn get(&self, path: &str) -> Result<Vec<u8>> {
            self.inner.get(path)
        }
        fn put(&self, path: &str, bytes: &[u8], expected: Option<&str>) -> Result<String> {
            if path == self.refuse {
                anyhow::bail!("the network went away");
            }
            self.inner.put(path, bytes, expected)
        }
        fn delete(&self, path: &str) -> Result<()> {
            self.inner.delete(path)
        }
    }

    #[test]
    fn one_file_failing_does_not_hold_up_the_rest() {
        // A photograph that timed out must not keep the sentence in the entry
        // beside it off the other machine.
        let dir = TempDir::new("sync-partial");
        write(&dir.0, "lazuli.yaml", "name: P
");
        write(&dir.0, "entries/one/entry.md", "a sentence
");
        write(&dir.0, "entries/one/photo.jpg", "pretend pixels
");
        let remote = Awkward {
            inner: Memory::default(),
            refuse: "entries/one/photo.jpg".into(),
        };

        let outcome = run(&dir.0, &remote).expect("the pass itself should not fail");

        assert_eq!(outcome.uploaded, 2);
        assert_eq!(outcome.failed, 1);
        assert!(outcome.problem.as_ref().is_some_and(|p| p.contains("photo.jpg")));
        assert!(remote.inner.files.borrow().contains_key("entries/one/entry.md"));
    }

    #[test]
    fn the_file_that_failed_is_tried_again_next_time() {
        let dir = TempDir::new("sync-retry");
        write(&dir.0, "entries/one/photo.jpg", "pretend pixels
");
        let remote = Awkward {
            inner: Memory::default(),
            refuse: "entries/one/photo.jpg".into(),
        };
        assert_eq!(run(&dir.0, &remote).expect("should run").failed, 1);

        // The obstacle goes away; nothing had to be told to retry.
        let willing = Memory::default();
        let outcome = run(&dir.0, &willing).expect("should run");
        assert_eq!(outcome.uploaded, 1);
        assert_eq!(outcome.failed, 0);
    }

    #[test]
    fn a_base_belonging_to_another_remote_is_dropped_rather_than_trusted() {
        // The bug this exists for: a base is a record of a conversation with one
        // remote. Applied to a different one, every file the new remote has not
        // got reads as something the other side deleted — and gets deleted here.
        // Measured against real Drive, where it removed a `lazuli.yaml`.
        let dir = TempDir::new("sync-rehomed");
        State {
            folder: "the-old-folder".into(),
            files: [(
                "lazuli.yaml".to_owned(),
                Agreed {
                    hash: "h".into(),
                    revision: "r".into(),
                },
            )]
            .into_iter()
            .collect(),
        }
        .write(&dir.0)
        .expect("should write");

        assert!(
            State::read(&dir.0, "a-different-folder").files.is_empty(),
            "a base for another remote must not be believed"
        );
        assert_eq!(State::read(&dir.0, "the-old-folder").files.len(), 1);
    }

    #[test]
    fn a_project_pointed_at_a_new_remote_uploads_rather_than_deletes() {
        let dir = TempDir::new("sync-rehome-run");
        write(&dir.0, "lazuli.yaml", "name: P
");
        let first = Memory::default();
        Config {
            folder: "first".into(),
        }
        .write(&dir.0)
        .expect("should write");
        run(&dir.0, &first).expect("should sync");
        assert_eq!(first.files.borrow().len(), 1);

        // Pointed somewhere else entirely, with the old base still on disk.
        Config {
            folder: "second".into(),
        }
        .write(&dir.0)
        .expect("should write");
        let second = Memory::default();
        let outcome = run(&dir.0, &second).expect("should sync");

        assert_eq!(outcome.deleted_here, 0, "nothing may be deleted locally");
        assert_eq!(outcome.uploaded, 1);
        assert!(local_path(&dir.0, "lazuli.yaml").exists());
    }

    #[test]
    fn the_sync_state_is_not_itself_synced() {
        let dir = TempDir::new("sync-state");
        write(&dir.0, "lazuli.yaml", "name: P\n");
        let remote = Memory::default();
        run(&dir.0, &remote).expect("should sync");

        assert!(dir.0.join(STATE_DIR).join(STATE_FILE).is_file());
        assert!(!remote.files.borrow().contains_key(".lazuli/sync-state.json"));
    }

    #[test]
    fn both_sides_writing_the_same_bytes_is_not_a_conflict() {
        let dir = TempDir::new("sync-same");
        let remote = Memory::default();
        write(&dir.0, "entries/one/entry.md", "the same\n");
        remote.put("entries/one/entry.md", b"the same\n", None).expect("put");

        let outcome = run(&dir.0, &remote).expect("should sync");

        assert_eq!(outcome.conflicted, 0);
        assert!(outcome.did_nothing());
    }

    #[test]
    fn a_real_disagreement_leaves_their_copy_beside_ours() {
        let dir = TempDir::new("sync-conflict");
        let remote = Memory::default();
        write(&dir.0, "entries/one/entry.md", "mine\n");
        remote.put("entries/one/entry.md", b"theirs\n", None).expect("put");

        let outcome = run(&dir.0, &remote).expect("should sync");

        assert_eq!(outcome.conflicted, 1);
        // Ours is untouched, and ours is what the remote now holds: the other
        // device must not be asked the same question all over again.
        assert_eq!(read(&dir.0, "entries/one/entry.md"), "mine\n");
        assert_eq!(remote.get("entries/one/entry.md").expect("get"), b"mine\n");

        // Theirs is beside it, under the name the conflict card recognises.
        let dropped: Vec<_> = fs::read_dir(dir.0.join("entries").join("one"))
            .expect("should list")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("sync-conflict"))
            .collect();
        assert_eq!(dropped.len(), 1);
        assert!(crate::conflicts::of_entry(
            &dir.0.join("entries").join("one"),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\n---\n\nmine\n"
        )
        .is_some());
    }

    // --- the one that matters ----------------------------------------------

    #[test]
    fn two_devices_converge() {
        // Two folders and one remote, edited in turn without either seeing the
        // other, then reconciled. They must end up holding the same bytes, and
        // nothing either of them wrote may have gone missing.
        let one = TempDir::new("sync-a");
        let two = TempDir::new("sync-b");
        let remote = Memory::default();

        // One starts the project and pushes it.
        write(&one.0, "lazuli.yaml", "name: Shared\n");
        write(&one.0, "entries/aaa/entry.md", "one's first\n");
        run(&one.0, &remote).expect("sync one");

        // Two picks it up.
        run(&two.0, &remote).expect("sync two");
        assert_eq!(read(&two.0, "entries/aaa/entry.md"), "one's first\n");

        // Now both work offline. Different entries: the UUID folders are what
        // make this the ordinary case rather than a collision.
        write(&one.0, "entries/bbb/entry.md", "one's second\n");
        write(&two.0, "entries/ccc/entry.md", "two's second\n");
        // And two deletes the entry one made first.
        fs::remove_file(local_path(&two.0, "entries/aaa/entry.md")).expect("remove");

        // Two syncs first, then one.
        run(&two.0, &remote).expect("sync two");
        run(&one.0, &remote).expect("sync one");
        // A second round, so each has seen what the other did.
        run(&two.0, &remote).expect("sync two");
        run(&one.0, &remote).expect("sync one");

        let after_one = scan_local(&one.0).expect("scan one");
        let after_two = scan_local(&two.0).expect("scan two");
        assert_eq!(after_one, after_two, "the two folders hold the same bytes");

        // Both new entries survived, and the deletion took.
        assert_eq!(read(&one.0, "entries/bbb/entry.md"), "one's second\n");
        assert_eq!(read(&one.0, "entries/ccc/entry.md"), "two's second\n");
        assert!(!local_path(&one.0, "entries/aaa/entry.md").exists());

        // And everything has settled: another pass on either side does nothing.
        assert!(run(&one.0, &remote).expect("sync one").did_nothing());
        assert!(run(&two.0, &remote).expect("sync two").did_nothing());
    }

    #[test]
    fn two_devices_editing_one_entry_both_keep_a_copy() {
        let one = TempDir::new("sync-both-a");
        let two = TempDir::new("sync-both-b");
        let remote = Memory::default();

        write(&one.0, "entries/aaa/entry.md", "as it was\n");
        run(&one.0, &remote).expect("sync one");
        run(&two.0, &remote).expect("sync two");

        // Both edit the same sentence, offline.
        write(&one.0, "entries/aaa/entry.md", "one's version\n");
        write(&two.0, "entries/aaa/entry.md", "two's version\n");

        run(&one.0, &remote).expect("sync one");
        let outcome = run(&two.0, &remote).expect("sync two");

        // Two is the one who finds a disagreement, and keeps both.
        assert_eq!(outcome.conflicted, 1);
        assert_eq!(read(&two.0, "entries/aaa/entry.md"), "two's version\n");
        let kept: Vec<_> = fs::read_dir(local_path(&two.0, "entries/aaa"))
            .expect("list")
            .filter_map(|entry| entry.ok())
            .map(|entry| fs::read_to_string(entry.path()).unwrap_or_default())
            .collect();
        assert!(kept.iter().any(|text| text == "one's version\n"));
        assert!(kept.iter().any(|text| text == "two's version\n"));
    }
}

// --- whether this machine syncs this project -------------------------------

const CONFIG_FILE: &str = "sync.json";

/// Which remote a project is synced to, from this device.
///
/// Per device and never synced, like the base beside it: "does this laptop sync
/// this project" is a fact about the laptop, the same argument `library.rs`
/// makes about which tab a project is filed under. A second device does not
/// need it carried across — it finds the project in the account and says so
/// itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The Drive folder this project lives in, or empty when it syncs nowhere.
    #[serde(default)]
    pub folder: String,
}

impl Config {
    pub fn read(root: &Path) -> Self {
        fs::read_to_string(root.join(STATE_DIR).join(CONFIG_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, root: &Path) -> Result<()> {
        let directory = root.join(STATE_DIR);
        fs::create_dir_all(&directory)
            .with_context(|| format!("creating {}", directory.display()))?;
        let text = serde_json::to_string_pretty(self).context("serialising the sync config")?;
        atomic::write(&directory.join(CONFIG_FILE), text)
    }

    pub fn is_on(&self) -> bool {
        !self.folder.is_empty()
    }
}

/// Stop syncing this project, and forget what was agreed with the remote.
///
/// The base goes with the setting: turned on again later, the project is
/// compared afresh rather than against an account of a conversation that
/// stopped some time ago.
pub fn turn_off(root: &Path) -> Result<()> {
    Config::default().write(root)?;
    State::default().write(root)
}

#[cfg(test)]
mod config_tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("lazuli-{label}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("should create a temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_project_syncs_nowhere_until_it_is_told_to() {
        let dir = TempDir::new("sync-off");
        assert!(!Config::read(&dir.0).is_on());
    }

    #[test]
    fn the_setting_survives_being_written_and_read() {
        let dir = TempDir::new("sync-on");
        Config {
            folder: "drive-folder-id".into(),
        }
        .write(&dir.0)
        .expect("should write");
        assert_eq!(Config::read(&dir.0).folder, "drive-folder-id");
        assert!(Config::read(&dir.0).is_on());
    }

    #[test]
    fn turning_it_off_forgets_what_was_agreed() {
        // Turned on again a month later, the project is compared afresh rather
        // than against an account of a conversation that stopped long ago.
        let dir = TempDir::new("sync-off-again");
        Config {
            folder: "f".into(),
        }
        .write(&dir.0)
        .expect("should write");
        State {
            folder: "f".into(),
            files: [(
                "a".to_owned(),
                Agreed {
                    hash: "h".into(),
                    revision: "r".into(),
                },
            )]
            .into_iter()
            .collect(),
        }
        .write(&dir.0)
        .expect("should write");

        turn_off(&dir.0).expect("should turn off");

        assert!(!Config::read(&dir.0).is_on());
        assert!(State::read(&dir.0, "f").files.is_empty());
    }

    #[test]
    fn neither_the_setting_nor_the_base_travels() {
        assert!(!is_synced(".lazuli/sync.json"));
        assert!(!is_synced(".lazuli/sync-state.json"));
    }
}
