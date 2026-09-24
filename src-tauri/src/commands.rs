//! The Tauri command surface, and the state behind it.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Local, NaiveDate};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::atomic;
use crate::authors;
use crate::conflicts;
use crate::dates;
use crate::library::{self, Slot};
use crate::model::{is_image, DateFormat, Project, ProjectMeta, SortOrder};
use crate::paths::{folder_name_for, is_named_after, unique_path};
use crate::store::{self, ProjectStore};
use crate::theme;
use crate::thumbs;
use crate::trashcan;
use crate::watch::{self, ProjectWatcher};

/// Emitted whenever a rescan finds the project has actually changed.
const PROJECT_CHANGED: &str = "project-changed";

/// Errors as the frontend sees them: a flat message, with the whole `anyhow`
/// chain rendered so a failure names its cause rather than just its symptom.
#[derive(Debug, Serialize)]
pub struct CmdError(String);

impl<E: Into<anyhow::Error>> From<E> for CmdError {
    fn from(err: E) -> Self {
        CmdError(format!("{:#}", err.into()))
    }
}

type CmdResult<T> = std::result::Result<T, CmdError>;

/// One deleted thing, remembered so Ctrl+Z can put it back.
#[derive(Debug, Clone)]
struct Trashed {
    /// The deletion's id in the project's own trash folder.
    ///
    /// An id rather than a path because the file is still in the project: this
    /// session's stack is only a shortcut to the most recent deletions, and the
    /// trash view can reach the same ones from any device.
    id: String,
    /// What it was, so the toast can say what came back.
    what: Deleted,
    /// The entry whose `image:` field was cleared by the delete, if any, so
    /// undo restores the choice and not merely the file.
    cleared_choice_for: Option<String>,
    /// Set when the delete cleared the project's chosen cover.
    cleared_cover: bool,
}

/// What a deletion was, for the sentence the undo puts on screen.
#[derive(Debug, Clone)]
enum Deleted {
    /// An entry folder. Named by a UUID, which is no use in a toast, and which
    /// nothing can collide with — so it always comes back under its own name.
    Entry,
    /// An image, under the filename it had, so undo can say when it had to come
    /// back under a different one.
    File(String),
}

/// The currently open project.
struct OpenProject {
    store: ProjectStore,
    /// What the frontend was last told. The diff baseline.
    snapshot: Project,
    /// Held so the watcher stays alive. Taken and stopped by [`Self::close`].
    watcher: Option<ProjectWatcher>,
    undo: Vec<Trashed>,
}

impl OpenProject {
    /// Stop watching, and wait until the watcher has actually stopped.
    ///
    /// `Debouncer`'s own `Drop` raises a stop flag and returns without joining
    /// the thread, so letting the value fall out of scope leaves the folder's
    /// directory handle open for as long as that thread takes to notice. That
    /// handle is opened with delete sharing and does not stop the folder being
    /// moved (measured: a project renames fine while it is being watched), but
    /// a rescan racing a delete is still a rescan of a folder that is going
    /// away, so the close waits for the thread rather than hoping.
    fn close(mut self) {
        let root = self.store.root().to_path_buf();
        if let Some(mut watcher) = self.watcher.take() {
            // Unwatch first: stopping the debouncer ends its own thread, but the
            // directory handle belongs to the watcher underneath it and is only
            // closed by asking for the path to be dropped.
            let _ = watcher.unwatch(&root);
            watcher.stop();
        }
    }
}

/// Close whatever project is open, if any, waiting for its watcher to stop.
fn close_open(app: &AppHandle) {
    let closing = app
        .state::<AppState>()
        .open
        .lock()
        .expect("project lock was poisoned")
        .take();
    // Outside the lock: `stop` blocks, and a rescan may be waiting on it.
    if let Some(open) = closing {
        open.close();
    }
}

#[derive(Default)]
pub struct AppState {
    open: Mutex<Option<OpenProject>>,
    /// Held for the whole of an open, so opens finish in the order they were
    /// asked for. They run off the main thread and would otherwise overlap:
    /// two quick clicks are two opens in flight, and the frontend keeps the
    /// one it asked for last, so that must also be the one left open here.
    opening: Mutex<()>,
    /// What each listed project last read as, so drawing the launch screen
    /// does not re-read every `lazuli.yaml` in the library. Same rules as the
    /// entry cache in [`ProjectStore`]: checked against the disk before it is
    /// used, and worth nothing but time if thrown away.
    listed: Mutex<HashMap<PathBuf, CachedListing>>,
}

/// One row of the launch screen as it last read, and the stamps it read at.
struct CachedListing {
    cached_at: SystemTime,
    modified: Option<SystemTime>,
    len: u64,
    listed: ListedProject,
}

/// Cap on the undo stack. Session-only anyway; this just stops a long tidying
/// session from growing it without bound.
const MAX_UNDO: usize = 50;

// --- opening and closing -------------------------------------------------

// Opening reads every entry in the folder, so it runs off the main thread: see
// [`off_thread`]. Blocking the main thread here froze the launch screen for the
// length of the scan, and queued every other command behind it.
#[tauri::command]
pub async fn open_project(app: AppHandle, path: PathBuf) -> CmdResult<Project> {
    Ok(off_thread(move || open_at(&app, path)).await?)
}

/// Where a project called `name` would go inside `parent`, and what stands in
/// the way if anything does.
///
/// The dialog calls this on every keystroke so the destination path is visible
/// as it is typed, and "that folder is taken" arrives before the click rather
/// than after it.
#[derive(Debug, Serialize)]
pub struct NewProjectTarget {
    pub path: PathBuf,
    /// A sentence to show under the field, or `None` when the path is usable.
    pub problem: Option<String>,
}

#[tauri::command]
pub fn new_project_target(parent: PathBuf, folder: String) -> NewProjectTarget {
    let path = parent.join(folder_name_for(&folder));
    NewProjectTarget {
        problem: store::new_project_problem(&path),
        path,
    }
}

/// Create a project in a *new* folder inside `parent`, named after the project.
///
/// `folder` and `name` are both given, and are the same string only when the
/// name is plain. A project's name is Markdown — `**Test** Test` — and a folder
/// should be called what that name *reads* as rather than how it is written;
/// only the frontend can strip the markers, so only the frontend can say what
/// the folder is called. Sanitising it for the filesystem still happens here,
/// so the path is the one [`new_project_target`] previewed from the same string.
///
/// `tab` is the tab that was on screen when the project was asked for, which
/// is where it is filed: creating one while looking at Wip puts it in Wip.
#[tauri::command]
pub async fn create_project(
    app: AppHandle,
    parent: PathBuf,
    folder: String,
    name: String,
    start_date: NaiveDate,
    tab: usize,
) -> CmdResult<Project> {
    let path = parent.join(folder_name_for(&folder));
    Ok(off_thread(move || {
        store::create_project(&path, &name, start_date)?;
        remember_projects_dir(&app, &path);
        let project = open_at(&app, path.clone())?;
        library::update(&app, |library| {
            library.insert(path, Slot { tab, index: 0 })
        });
        Ok(project)
    })
    .await?)
}

fn open_at(app: &AppHandle, path: PathBuf) -> Result<Project> {
    let state = app.state::<AppState>();
    let _one_at_a_time = state.opening.lock().expect("opening lock was poisoned");

    if !store::is_project(&path) {
        bail!(
            "{} is not a Lazuli project (no {} inside)",
            path.display(),
            store::META_FILE
        );
    }

    // Before anything reads the folder, and before the watcher exists to see
    // it happen: a project written by a build from before the rename gets its
    // marker file renamed here, once.
    store::migrate_meta(&path)?;

    // Also before the watcher: an expired deletion hands its contents on to the
    // system Recycle Bin here, which is a write the watcher would otherwise see
    // and rescan for. Opening the project is the only moment the app reliably
    // gets, and a sweep costs nothing when there is nothing to sweep.
    trashcan::purge_expired(&path);

    // Without this the webview silently refuses to load any image in the
    // folder: `file://` is blocked, and `convertFileSrc` only works for paths
    // the asset scope allows.
    app.asset_protocol_scope()
        .allow_directory(&path, true)
        .with_context(|| format!("allowing asset access to {}", path.display()))?;

    let mut store = ProjectStore::new(&path);
    let snapshot = store.scan()?;
    let watcher = watch::watch_project(app.clone(), &path)?;

    // Behind the open rather than inside it: a project of real photographs that
    // has never had thumbnails takes seconds to build them, and the timeline
    // should be on screen for all of it. Cards fall back to the full-size
    // picture until each one lands, which is what every card did until now.
    let building = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let made = thumbs::build_all(&building);
        if made > 0 && cfg!(debug_assertions) {
            eprintln!("lazuli: built {made} thumbnails in {}", building.display());
        }
    });

    // Listed if it is not already, and left exactly where it is if it is: the
    // order on the launch screen is the user's arrangement, and opening a
    // project is not a request to rearrange it.
    library::update(app, |library| library.ensure(&path));
    // Swapped in one step, so there is never a moment with no project open
    // that another thread could see. The project being replaced then gets the
    // same orderly shutdown as a close — outside the lock, because `close`
    // waits for a watcher thread that may itself be waiting for the lock.
    let replaced = state
        .open
        .lock()
        .expect("project lock was poisoned")
        .replace(OpenProject {
            store,
            snapshot: snapshot.clone(),
            watcher: Some(watcher),
            undo: Vec::new(),
        });
    if let Some(open) = replaced {
        open.close();
    }
    Ok(snapshot)
}

// Async for the same reason as [`open_project`]: closing waits for the watcher
// thread to stop.
#[tauri::command]
pub async fn close_project(app: AppHandle) -> CmdResult<()> {
    off_thread(move || {
        close_open(&app);
        Ok(())
    })
    .await?;
    Ok(())
}

/// Re-read the project and tell the frontend only if something actually
/// differs. Called by the watcher and after every write.
/// Read the open project's folder again, because the window has come back.
///
/// The watcher is the usual way a change is noticed, and it is not something
/// to stake correctness on having survived: a sleep, a drive that went away
/// and came back, a syncer writing while the window sat behind another one.
/// On a phone it is not even the usual way — the process is frozen or killed
/// the moment it is backgrounded, which is exactly when a syncer runs.
///
/// Off the main thread because it reads, and cheap when nothing has changed:
/// every entry folder still costs two stats, and no more than that. Emits
/// nothing when the folder says what it said before.
#[tauri::command]
pub async fn rescan_now(app: AppHandle) -> CmdResult<()> {
    Ok(off_thread(move || {
        rescan_and_emit(&app);
        Ok(())
    })
    .await?)
}

pub fn rescan_and_emit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut guard = state.open.lock().expect("project lock was poisoned");
    let Some(open) = guard.as_mut() else {
        return;
    };
    match open.store.scan() {
        Ok(fresh) => {
            if fresh != open.snapshot {
                open.snapshot = fresh.clone();
                let _ = app.emit(PROJECT_CHANGED, fresh);
            }
        }
        Err(err) => eprintln!("lazuli: rescan failed: {err:#}"),
    }
}

/// Run a write against the open project, then reconcile.
///
/// Every mutating command goes through this so there is one place where the
/// lock is taken and one place that triggers the diff.
fn with_project<T>(
    app: &AppHandle,
    state: &State<AppState>,
    action: impl FnOnce(&mut OpenProject) -> Result<T>,
) -> Result<T> {
    let outcome = {
        let mut guard = state.open.lock().expect("project lock was poisoned");
        let open = guard
            .as_mut()
            .ok_or_else(|| anyhow!("no project is open"))?;
        action(open)?
    };
    // Outside the lock: `rescan_and_emit` takes it again.
    rescan_and_emit(app);
    Ok(outcome)
}

fn root_of(open: &OpenProject) -> PathBuf {
    open.store.root().to_path_buf()
}

// --- project metadata ----------------------------------------------------

/// Rename the project, and the folder it lives in along with it.
///
/// `name` is what the project is called and goes in `lazuli.yaml`; `folder` is
/// what that name reads as with its Markdown markers stripped, and is what the
/// folder is called. `was_folder` is the same for the name being replaced,
/// which is what says whether the folder on disk was named after the project at
/// all — see [`follow_with_folder`]. Both come from the frontend because
/// stripping Markdown is the frontend's to do.
///
/// Async because the rename closes and reopens the project, which waits for the
/// watcher thread.
#[tauri::command]
pub async fn set_project_name(
    app: AppHandle,
    name: String,
    folder: String,
    was_folder: String,
) -> CmdResult<()> {
    Ok(off_thread(move || {
        let state = app.state::<AppState>();
        let root = with_project(&app, &state, |open| {
            let root = root_of(open);
            let mut meta = store::read_meta(&root)?;
            meta.name = name.clone();
            store::write_meta(&root, &meta)?;
            Ok(root)
        })?;
        follow_with_folder(&app, &root, &was_folder, &folder)
    })
    .await?)
}

/// Move the project folder so it still matches the project's name.
///
/// Only when the folder was named after the project to begin with. A project
/// kept at the root of its own repository, or in a folder the user named
/// themselves, stays where it is: typing in the title is a rename of the
/// journal, and it should not silently rename someone's source tree.
///
/// Both names here are folder names — the project's name with its Markdown
/// stripped — and not the names in `lazuli.yaml`.
fn follow_with_folder(app: &AppHandle, root: &Path, was_called: &str, name: &str) -> Result<()> {
    let Some(parent) = root.parent() else {
        return Ok(());
    };
    let current = file_name_of(root);
    if !is_named_after(&current, was_called) {
        return Ok(());
    }
    let desired = folder_name_for(name);
    if desired == current {
        return Ok(());
    }

    let direct = parent.join(&desired);
    // A change of case only: Windows says the destination already exists,
    // because it *is* this folder, and `unique_path` would answer with a
    // ` (2)`. Compare the real paths rather than the spellings.
    let target = if fs::canonicalize(&direct).ok() == fs::canonicalize(root).ok() {
        direct
    } else {
        // Nothing is ever overwritten, here as everywhere else.
        unique_path(parent, &desired)
    };

    // Closed first, so no rescan runs against a folder that is moving. The
    // session's undo stack goes with it: the paths it holds are all about to
    // stop existing.
    close_open(app);
    let moved = fs::rename(root, &target)
        .with_context(|| format!("renaming {} to {}", root.display(), target.display()));
    // Before the reopen, which lists whatever it opens: the same project under
    // a new path, so the row follows the folder instead of being re-filed at
    // the top of the first tab.
    if moved.is_ok() {
        library::update(app, |library| {
            library.replace_path(root, target.clone())
        });
    }
    // Something has to be open either way — the page is showing a project. When
    // the move failed, the folder is still at the path it was opened from.
    let reopened = open_at(
        app,
        if moved.is_ok() {
            target.clone()
        } else {
            root.to_path_buf()
        },
    );
    moved?;
    let project = reopened?;
    // `open_at` returns the snapshot rather than emitting it, and the page is
    // still holding the old root — which every image URL is built from.
    let _ = app.emit(PROJECT_CHANGED, project);
    Ok(())
}

/// How this project's dates are read. A property of the project: see
/// [`DateFormat`].
#[tauri::command]
pub fn set_date_format(
    app: AppHandle,
    state: State<AppState>,
    format: DateFormat,
) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let mut meta = store::read_meta(&root)?;
        meta.date_format = format;
        store::write_meta(&root, &meta)
    })?;
    Ok(())
}

/// Show the spelling suggestions for the word the caret is in.
///
/// All this does is press the Menu key; the webview opens its own context
/// menu, which is the only place the suggestions exist. See [`crate::keys`].
#[tauri::command]
pub fn show_spelling_suggestions() -> CmdResult<()> {
    crate::keys::press_context_menu_key()?;
    Ok(())
}

/// Which end of the timeline this project is read from. A property of the
/// project: see [`SortOrder`].
#[tauri::command]
pub fn set_sort_order(
    app: AppHandle,
    state: State<AppState>,
    order: SortOrder,
) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let mut meta = store::read_meta(&root)?;
        meta.sort_order = order;
        store::write_meta(&root, &meta)
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_start_date(
    app: AppHandle,
    state: State<AppState>,
    start_date: NaiveDate,
) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let mut meta = store::read_meta(&root)?;
        meta.start_date = start_date;
        store::write_meta(&root, &meta)
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_cover(
    app: AppHandle,
    state: State<AppState>,
    filename: Option<String>,
) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let mut meta = store::read_meta(&root)?;
        meta.cover = filename;
        store::write_meta(&root, &meta)
    })?;
    Ok(())
}

// --- entries -------------------------------------------------------------

/// Add an entry for `date`, defaulting to the journal day in progress.
///
/// The 5am rule lives in that default: opening the app at 02:00 offers
/// yesterday, which is nearly always the day the work belongs to.
#[tauri::command]
pub fn create_entry(
    app: AppHandle,
    state: State<AppState>,
    date: Option<NaiveDate>,
) -> CmdResult<String> {
    let date = date.unwrap_or_else(dates::today);
    let avatar = avatar_path(&app);
    Ok(with_project(&app, &state, |open| {
        let root = root_of(open);
        let me = author(&app);
        // Writing to the project is what publishes the name, so that a project
        // only ever read does not gain a folder naming whoever looked at it.
        if let Some((id, name)) = &me {
            authors::publish(&root, id, name, avatar.as_deref());
        }
        store::create_entry(
            &root,
            date,
            Local::now().fixed_offset(),
            me.as_ref().map(|(id, _)| id.as_str()),
        )
    })?)
}

#[tauri::command]
pub fn update_entry(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    date: Option<NaiveDate>,
    text: Option<String>,
    // `Some(None)` clears the chosen image; `None` leaves it alone. Serde maps
    // an absent field to the outer `None` and an explicit `null` to the inner.
    image: Option<Option<String>>,
) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        store::update_entry(
            &root_of(open),
            &id,
            date,
            text.as_deref(),
            image.as_ref().map(|inner| inner.as_deref()),
        )
    })?;
    Ok(())
}

// --- importing images ----------------------------------------------------

/// Copy files into an entry folder, or into `cover/` when `entry_id` is absent.
///
/// Returns the names they ended up with, which may differ from the source
/// names when something was already using them.
#[tauri::command]
pub fn import_images(
    app: AppHandle,
    state: State<AppState>,
    entry_id: Option<String>,
    sources: Vec<PathBuf>,
) -> CmdResult<Vec<String>> {
    Ok(with_project(&app, &state, |open| {
        let target = target_dir(open, entry_id.as_deref())?;
        fs::create_dir_all(&target)
            .with_context(|| format!("creating {}", target.display()))?;

        let mut named = Vec::new();
        for source in &sources {
            let Some(filename) = source.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_image(filename) {
                continue;
            }
            let destination = unique_path(&target, filename);
            fs::copy(source, &destination).with_context(|| {
                format!("copying {} to {}", source.display(), destination.display())
            })?;
            // Built as the picture arrives, so the timeline never has to decode
            // the full-size one to draw a card.
            thumbs::build_one(&root_of(open), &destination);
            named.push(file_name_of(&destination));
        }
        if let Some(last) = named.last() {
            choose_image(open, entry_id.as_deref(), last)?;
        }
        Ok(named)
    })?)
}

/// Make a newly added image the chosen one.
///
/// Adding an image is how you say which picture the day gets, so the new one
/// wins rather than joining the pile unseen. Nothing is lost: the previous
/// choice is still in the folder, one click away in the picker. On a batch the
/// last file added wins, being the most recent thing the user did.
fn choose_image(open: &OpenProject, entry_id: Option<&str>, filename: &str) -> Result<()> {
    let root = root_of(open);
    match entry_id {
        Some(id) => store::update_entry(&root, id, None, None, Some(Some(filename))),
        None => {
            let mut meta = store::read_meta(&root)?;
            meta.cover = Some(filename.to_owned());
            store::write_meta(&root, &meta)
        }
    }
}

/// Save pasted image bytes into an entry folder or `cover/`.
#[tauri::command]
pub fn import_image_bytes(
    app: AppHandle,
    state: State<AppState>,
    entry_id: Option<String>,
    filename: String,
    bytes: Vec<u8>,
) -> CmdResult<String> {
    Ok(with_project(&app, &state, |open| {
        // A pasted file keeps the name it had in Explorer, so this one comes
        // from outside the app. The rules for a name a filesystem will accept
        // are the same whether it ends up on a folder or on a file, separators
        // included — a `..\` here would otherwise write outside the project.
        let filename = folder_name_for(&filename);
        if !is_image(&filename) {
            bail!("{filename} is not a recognised image type");
        }
        let target = target_dir(open, entry_id.as_deref())?;
        fs::create_dir_all(&target)
            .with_context(|| format!("creating {}", target.display()))?;
        let destination = unique_path(&target, &filename);
        atomic::write(&destination, &bytes)
            .with_context(|| format!("writing {}", destination.display()))?;
        thumbs::build_one(&root_of(open), &destination);
        let saved = file_name_of(&destination);
        choose_image(open, entry_id.as_deref(), &saved)?;
        Ok(saved)
    })?)
}

/// Where an image belongs: an entry's own folder, or the project's `cover/`.
fn target_dir(open: &OpenProject, entry_id: Option<&str>) -> Result<PathBuf> {
    let root = root_of(open);
    Ok(match entry_id {
        Some(id) => {
            let dir = store::entry_dir(&root, id);
            if !dir.is_dir() {
                bail!("no entry {id} in this project");
            }
            dir
        }
        None => root.join(store::COVER_DIR),
    })
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned()
}

// --- deleting and undo ---------------------------------------------------

/// Move one image into the project's trash.
///
/// Deleting the chosen image clears the choice rather than promoting one of the
/// remaining candidates: the app has no way to know which attempt was meant.
#[tauri::command]
pub fn trash_image(
    app: AppHandle,
    state: State<AppState>,
    entry_id: Option<String>,
    filename: String,
) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let dir = target_dir(open, entry_id.as_deref())?;
        let path = dir.join(&filename);
        if !path.is_file() {
            bail!("{} is already gone", path.display());
        }

        let mut cleared_choice_for = None;
        let mut cleared_cover = false;

        match entry_id.as_deref() {
            Some(id) => {
                let entry_file = dir.join(store::ENTRY_FILE);
                let (frontmatter, _) = store::read_entry_file(&entry_file)?;
                if frontmatter.image.as_deref() == Some(filename.as_str()) {
                    store::update_entry(&root, id, None, None, Some(None))?;
                    cleared_choice_for = Some(id.to_owned());
                }
            }
            None => {
                let mut meta = store::read_meta(&root)?;
                if meta.cover.as_deref() == Some(filename.as_str()) {
                    meta.cover = None;
                    store::write_meta(&root, &meta)?;
                    cleared_cover = true;
                }
            }
        }

        let deletion = trashcan::put(&root, &path, None)?;
        push_undo(
            open,
            Trashed {
                id: deletion,
                what: Deleted::File(filename.clone()),
                cleared_choice_for,
                cleared_cover,
            },
        );
        Ok(())
    })?;
    Ok(())
}

/// Move a whole entry folder, images and all, into the project's trash.
#[tauri::command]
pub fn trash_entry(app: AppHandle, state: State<AppState>, id: String) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let dir = store::entry_dir(&root, &id);
        if !dir.is_dir() {
            bail!("no entry {id} in this project");
        }
        let deletion = trashcan::put(&root, &dir, None)?;
        push_undo(
            open,
            Trashed {
                id: deletion,
                what: Deleted::Entry,
                cleared_choice_for: None,
                cleared_cover: false,
            },
        );
        Ok(())
    })?;
    Ok(())
}

fn push_undo(open: &mut OpenProject, record: Trashed) {
    open.undo.push(record);
    if open.undo.len() > MAX_UNDO {
        open.undo.remove(0);
    }
}

/// What an undo did, so the UI can say so.
#[derive(Debug, Serialize)]
pub struct UndoOutcome {
    /// Human-readable summary for the toast.
    pub message: String,
    /// True when there is still something further back to undo.
    pub more: bool,
}

/// Restore the most recent deletion.
#[tauri::command]
pub fn undo_delete(app: AppHandle, state: State<AppState>) -> CmdResult<Option<UndoOutcome>> {
    Ok(with_project(&app, &state, |open| {
        let Some(record) = open.undo.pop() else {
            return Ok(None);
        };
        let root = root_of(open);
        let message = match trashcan::restore(&root, &record.id) {
            Ok(restored_as) => {
                // Re-point the entry or cover at the file, under whatever name
                // it came back as: clearing the choice was part of the delete,
                // so undoing must put it back too.
                if let Some(id) = &record.cleared_choice_for {
                    store::update_entry(&root, id, None, None, Some(Some(&restored_as)))?;
                }
                if record.cleared_cover {
                    let mut meta = store::read_meta(&root)?;
                    meta.cover = Some(restored_as.clone());
                    store::write_meta(&root, &meta)?;
                }

                match &record.what {
                    Deleted::Entry => "Restored the entry".to_owned(),
                    Deleted::File(name) if &restored_as == name => {
                        format!("Restored {restored_as}")
                    }
                    // The old name was taken in the meantime; say so rather
                    // than letting a file appear under a name nobody chose.
                    Deleted::File(_) => format!("Restored as {restored_as}"),
                }
            }
            // The trash was emptied by hand, or the deletion is old enough that
            // its contents have gone on to the system Recycle Bin. The record is
            // already popped, so a second Ctrl+Z moves further back.
            Err(err) => format!("Could not undo: {err:#}"),
        };
        Ok(Some(UndoOutcome {
            message,
            more: !open.undo.is_empty(),
        }))
    })?)
}

/// Run blocking filesystem work away from the main thread.
///
/// A `#[tauri::command] fn` runs on the main thread, where a Recycle Bin move
/// taking a second or two, or a scan of a large project, blocks every other
/// command behind it and freezes the page meanwhile. Deleting one project used
/// to empty the whole launch list until the move finished, because the
/// `recent_projects` call that redraws it was stuck in that queue.
pub async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .context("a filesystem task did not finish")?
}

// --- misc ----------------------------------------------------------------

/// A project folder named on the command line, so `lazuli <folder>` opens
/// straight into it. Also what makes dragging a folder onto the exe work.
#[tauri::command]
pub fn startup_project() -> Option<PathBuf> {
    std::env::args()
        .skip(1)
        // Tauri and cargo pass their own flags through; only bare paths that
        // actually are projects count.
        .filter(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .find(|path| store::is_project(path))
}

#[tauri::command]
pub fn journal_today() -> NaiveDate {
    dates::today()
}

/// Every project the app knows about, in the tabs the user filed them under.
///
/// The one piece of state that is not in a project folder, because it is about
/// the app rather than any one project. See [`library`].
///
/// Off the main thread like every other read of the disk: this is what draws
/// the launch screen, and it should never sit behind anything.
#[tauri::command]
pub async fn project_tabs(app: AppHandle) -> CmdResult<Vec<TabView>> {
    Ok(off_thread(move || Ok(read_tabs(&app))).await?)
}

fn read_tabs(app: &AppHandle) -> Vec<TabView> {
    let state = app.state::<AppState>();
    let mut cache = state.listed.lock().expect("the listing cache was poisoned");
    let mut listed = Vec::new();

    for tab in library::read(app).tabs {
        let mut projects = Vec::new();
        for path in tab.projects {
            // A folder that is no longer a project is skipped rather than
            // dropped: the file is the user's arrangement, and a disk that is
            // not mounted this morning is not a reason to rewrite it.
            if let Some(project) = listed_project(app, &mut cache, path) {
                projects.push(project);
            }
        }
        listed.push(TabView {
            name: tab.name,
            projects,
        });
    }

    // A project forgotten from the library takes its cache line with it.
    let still_listed: HashSet<&PathBuf> = listed
        .iter()
        .flat_map(|tab| tab.projects.iter().map(|project| &project.path))
        .collect();
    cache.retain(|path, _| still_listed.contains(path));

    listed
}

/// One launch-screen row, from the cache where the project's `lazuli.yaml` has
/// not changed and from the disk where it has.
///
/// `None` when the folder is not a project. Resolving the marker file answers
/// that *and* hands back the stamps to cache against, which is why it is one
/// look here where it used to be one for `is_project`, another for
/// `meta_path`, and a third to read the file.
fn listed_project(
    app: &AppHandle,
    cache: &mut HashMap<PathBuf, CachedListing>,
    path: PathBuf,
) -> Option<ListedProject> {
    let reading_at = SystemTime::now();
    let (marker, modified, len) = store::marker(&path)?;

    if let Some(cached) = cache.get(&path) {
        if store::settled(modified, cached.cached_at)
            && cached.modified == modified
            && cached.len == len
        {
            return Some(cached.listed.clone());
        }
    }

    let meta = store::read_meta_at(&marker).ok();
    // The launch screen shows each project behind its own cover, so the
    // webview has to be allowed to load that one folder. Not recursive,
    // and not the whole project: nothing else is shown until it opens.
    if meta.as_ref().is_some_and(|meta| meta.cover.is_some()) {
        let _ = app
            .asset_protocol_scope()
            .allow_directory(path.join(store::COVER_DIR), false);
    }
    let listed = ListedProject {
        name: meta
            .as_ref()
            .map(|meta| meta.name.clone())
            .unwrap_or_else(|| file_name_of(&path)),
        cover: meta.and_then(|meta| meta.cover),
        path: path.clone(),
    };
    cache.insert(
        path,
        CachedListing {
            cached_at: reading_at,
            modified,
            len,
            listed: listed.clone(),
        },
    );
    Some(listed)
}

#[derive(Debug, Serialize)]
pub struct TabView {
    pub name: String,
    pub projects: Vec<ListedProject>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListedProject {
    pub name: String,
    pub path: PathBuf,
    /// Filename within the project's `cover/`, when one is chosen.
    pub cover: Option<String>,
}

/// File a project folder without opening it.
///
/// What the launch screen's "Add project…" does: the folder joins the list and
/// the user picks their moment to go into it, rather than being taken there by
/// having pointed at it once.
#[tauri::command]
pub fn add_project(app: AppHandle, path: PathBuf, slot: Slot) -> CmdResult<()> {
    if !store::is_project(&path) {
        return Err(anyhow!(
            "{} is not a Lazuli project (no {} inside)",
            path.display(),
            store::META_FILE
        )
        .into());
    }
    library::update(&app, |library| {
        let slot = stored_slot(library, slot);
        library.insert(path, slot);
    });
    Ok(())
}

/// Put a project at a slot: the far end of a drag, within a tab or across two.
#[tauri::command]
pub fn move_project(app: AppHandle, path: PathBuf, slot: Slot) {
    library::update(&app, |library| {
        let slot = stored_slot(library, slot);
        library.insert(path, slot);
    });
}

/// A slot among the rows on screen, as a slot in the stored list.
///
/// The two differ whenever a tab holds a folder that is not a project just
/// now — an unplugged drive, a folder moved away behind the app's back. Those
/// are skipped when the list is drawn but kept in the file, so an index
/// counted off the screen would land somewhere else in it, and the row nobody
/// can see would drift a place every time its neighbours were rearranged.
///
/// Only the two commands that follow a pointer go through this. A slot that
/// came *from* the list — the one an undo puts a row back at — is already one
/// of these and must not be translated twice.
fn stored_slot(library: &library::Library, slot: Slot) -> Slot {
    let tab = slot.tab.min(library.tabs.len().saturating_sub(1));
    let projects = &library.tabs[tab].projects;
    let mut visible = 0;
    for (index, path) in projects.iter().enumerate() {
        if visible == slot.index {
            return Slot { tab, index };
        }
        if store::is_project(path) {
            visible += 1;
        }
    }
    Slot {
        tab,
        index: projects.len(),
    }
}

/// Drop a project from the list, returning where it was.
///
/// The slot comes back so undoing puts it back where it was rather than at
/// the top: the order is the user's, and forgetting one by mistake should not
/// rearrange anything.
#[tauri::command]
pub fn forget_project(app: AppHandle, path: PathBuf) -> Option<Slot> {
    library::update(&app, |library| library.remove(&path))
}

/// Put a forgotten project back at the slot it held.
#[tauri::command]
pub fn restore_listing(app: AppHandle, path: PathBuf, slot: Slot) {
    library::update(&app, |library| library.insert(path, slot));
}

/// Add a tab, returning where in the strip it landed.
#[tauri::command]
pub fn add_tab(app: AppHandle, name: String) -> usize {
    library::update(&app, |library| library.add_tab(name))
}

#[tauri::command]
pub fn rename_tab(app: AppHandle, index: usize, name: String) {
    library::update(&app, |library| library.rename_tab(index, name));
}

/// Remove a tab, handing whatever was in it to the first tab.
///
/// Returns the tab as it was, so the undo can put it back with its projects.
/// Nothing on disk moves: a tab is filing and only filing.
#[tauri::command]
pub fn delete_tab(app: AppHandle, index: usize) -> Option<library::Tab> {
    library::update(&app, |library| library.remove_tab(index))
}

#[tauri::command]
pub fn restore_tab(app: AppHandle, index: usize, tab: library::Tab) {
    library::update(&app, |library| library.insert_tab(index, tab));
}

#[tauri::command]
pub fn move_tab(app: AppHandle, from: usize, to: usize) {
    library::update(&app, |library| library.move_tab(from, to));
}

/// Move a whole project folder into the projects directory's trash.
///
/// One level up from the project's own `.lazuli-trash/`, because a folder
/// cannot go inside itself. The projects directory is the app's own territory —
/// it is where new projects are made — so a dotfolder there is unobjectionable,
/// and for a project kept in it the move is a rename. A project kept elsewhere,
/// such as a journal living in the repository it is about, may be on another
/// drive, and [`trashcan::put`] copies rather than renames in that case.
///
/// Returns its slot in the list, together with the deletion's id so the undo
/// can put both the folder and the listing back.
#[tauri::command]
pub async fn trash_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> CmdResult<Option<TrashedProject>> {
    if !store::is_project(&path) {
        return Err(anyhow!("{} is not a Lazuli project", path.display()).into());
    }

    // Closing first so no rescan runs against a folder that is on its way out.
    // The watcher's own handle does not block the move.
    if state
        .open
        .lock()
        .expect("project lock was poisoned")
        .as_ref()
        .is_some_and(|open| open.store.root() == path)
    {
        close_open(&app);
    }

    let trash_root = default_projects_dir(app.clone());
    let trashing = path.clone();
    let id = off_thread(move || trashcan::put(&trash_root, &trashing, None)).await?;
    Ok(forget_project(app, path).map(|slot| TrashedProject { id, slot }))
}

/// A deleted project, as the undo needs to remember it.
#[derive(Debug, Serialize, serde::Deserialize)]
pub struct TrashedProject {
    /// Its deletion in the projects directory's trash.
    pub id: String,
    /// Where it sat in the launch screen's list.
    pub slot: Slot,
}

/// Take a deleted project back out of the trash and back into the list.
#[tauri::command]
pub async fn restore_project(
    app: AppHandle,
    path: PathBuf,
    id: String,
    slot: Slot,
) -> CmdResult<PathBuf> {
    let trash_root = default_projects_dir(app.clone());
    let restored_as = off_thread(move || trashcan::restore(&trash_root, &id)).await?;
    // The old name may have been taken in the meantime, in which case the
    // folder comes back under a different one and the list must follow it.
    let actual = path.with_file_name(restored_as);
    restore_listing(app, actual.clone(), slot);
    Ok(actual)
}

/// Settle a conflicted entry by keeping one of its versions.
///
/// The versions are the ones the last scan offered, in that order. Whatever was
/// in `entry.md`, and any file a syncer left beside it, go to the project's
/// trash rather than being removed: choosing between two versions of a sentence
/// is exactly the kind of decision somebody wants back an hour later.
#[tauri::command]
pub fn resolve_conflict(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    version: usize,
) -> CmdResult<()> {
    let me = author(&app);
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let entry = open
            .snapshot
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| anyhow!("no entry {id} in this project"))?;
        let conflict = entry
            .conflict
            .as_ref()
            .ok_or_else(|| anyhow!("that entry is not in two minds"))?;
        conflicts::resolve(
            &root,
            &store::entry_dir(&root, &id),
            conflict,
            version,
            me.as_ref().map(|(id, _)| id.as_str()),
        )
    })?;
    Ok(())
}

/// Everything sitting in a project's trash, newest first.
#[tauri::command]
pub fn trash_contents(state: State<AppState>) -> Vec<trashcan::TrashedItem> {
    let open = state.open.lock().expect("project lock was poisoned");
    open.as_ref()
        .map(|open| trashcan::list(open.store.root()))
        .unwrap_or_default()
}

/// Put one thing back from the trash view, rather than from the undo stack.
///
/// The undo stack only holds what this session did; this reaches anything in
/// the folder, including a deletion that arrived from another device.
#[tauri::command]
pub fn restore_trashed(app: AppHandle, state: State<AppState>, id: String) -> CmdResult<String> {
    Ok(with_project(&app, &state, |open| {
        trashcan::restore(&root_of(open), &id)
    })?)
}

const SETTINGS_FILE: &str = "settings.json";

/// Who the user is, kept apart from everything else. See [`Settings`].
const ME_FILE: &str = "profile.json";

/// The fields of [`Settings`] that live in [`ME_FILE`], by their names on disk.
const ME_FIELDS: [&str; 4] = ["author_id", "author_name", "author_avatar", "not_me"];

/// Fields an earlier build wrote that nothing reads any more, and that are not
/// carried through a rewrite the way an unknown field is: the Google sign-in of
/// the builds that synced through Drive. A refresh token nothing uses is a
/// credential lying about, and the sooner it goes the better.
const RETIRED_FIELDS: [&str; 2] = ["drive_tokens", "drive_account"];

/// App-level settings, kept next to the project list rather than in any
/// project folder.
///
/// Stored in two files, and the split is the fix for a loss that kept
/// happening. `settings.json` has been written by every release, and 0.4.0
/// knows four fields of it: whenever it saved — and the theme is saved on every
/// launch — it wrote back those four and nothing else, taking the name, the
/// picture and the author id with it whenever a release and a newer build
/// shared a machine. So:
///
/// - What this machine looks like stays in `settings.json`, where every release
///   expects it.
/// - Who the user is lives in `profile.json`, which no earlier release knows
///   exists, and so none can rewrite.
/// - Both carry the fields they do not know through a rewrite, in `rest`, so
///   that the next time one build falls behind another, the older one cannot
///   strip what the newer wrote.
#[derive(Debug, Default)]
struct Settings {
    /// Where the "new project" dialog opens. Updated whenever a project is
    /// created somewhere else, so the app follows wherever you keep them.
    projects_dir: Option<PathBuf>,
    /// `system`, `light` or `dark`. The page keeps its own copy in
    /// `localStorage`, because it needs the theme before the first paint and a
    /// command is a round trip; this copy is the one Rust can read, and it
    /// exists so the *window* — its frame and the colour behind the page — is
    /// already right when it opens. See `theme.rs`.
    theme: Option<String>,
    /// The chosen ground of each theme, as `#rrggbb`. Stored for the same
    /// reason as `theme`, and useless without it: knowing the window should
    /// open dark says nothing about *which* dark the user picked.
    background_dark: Option<String>,
    background_light: Option<String>,
    /// Who this user is, minted on first use and stable from then on.
    ///
    /// Global rather than per project, and per *person* rather than per
    /// install: a second device is meant to end up with this same id, so that a
    /// phone and a laptop are one author rather than two collaborators. Nothing
    /// carries it across on its own — there is no account to — so a device
    /// opening a project whose authors it does not know asks which of them it
    /// is, and adopts the answer. See [`claim_author`].
    author_id: Option<String>,
    /// The name published into the projects this user writes to. Defaulted from
    /// the machine, and theirs to change.
    author_name: Option<String>,
    /// The picture's filename, inside the app's own config folder. Kept there
    /// rather than in any project, because it is the user's and not a
    /// project's; each project gets a copy when they write in it.
    author_avatar: Option<String>,
    /// Authors this user has said they are not, so they are asked once.
    ///
    /// Kept by author rather than by project: somebody who shares three
    /// projects with you is somebody you should only have to say "not me" about
    /// once.
    not_me: Vec<String>,
    /// What `settings.json` held that this build does not know.
    machine_rest: serde_json::Map<String, serde_json::Value>,
    /// What `profile.json` held that this build does not know.
    me_rest: serde_json::Map<String, serde_json::Value>,
}

/// `settings.json` as it is on disk.
#[derive(Debug, Default, Serialize, serde::Deserialize)]
struct MachineFile {
    #[serde(default)]
    projects_dir: Option<PathBuf>,
    #[serde(default)]
    theme: Option<String>,
    #[serde(default)]
    background_dark: Option<String>,
    #[serde(default)]
    background_light: Option<String>,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

/// `profile.json` as it is on disk.
#[derive(Debug, Default, Serialize, serde::Deserialize)]
struct MeFile {
    #[serde(default)]
    author_id: Option<String>,
    #[serde(default)]
    author_name: Option<String>,
    #[serde(default)]
    author_avatar: Option<String>,
    #[serde(default)]
    not_me: Vec<String>,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

/// The user's own profile, as the button and the dialog show it.
#[derive(Debug, Serialize)]
pub struct MyProfile {
    pub name: String,
    /// The picture as a `data:` URL, or `None` for the default drawing.
    ///
    /// Inlined rather than served as a file: the picture lives in the app's
    /// config folder, which the webview has no access to, and opening that
    /// folder to it to show one small image would be a wide door for a narrow
    /// need.
    pub avatar: Option<String>,
}

/// Where the user's own picture is kept, when they have one.
fn avatar_path(app: &AppHandle) -> Option<PathBuf> {
    let name = read_settings(app).author_avatar?;
    let path = app.path().app_config_dir().ok()?.join(name);
    path.is_file().then_some(path)
}

#[tauri::command]
pub fn my_profile(app: AppHandle) -> MyProfile {
    let settings = read_settings(&app);
    MyProfile {
        name: settings
            .author_name
            .unwrap_or_else(authors::name_from_the_machine),
        avatar: avatar_path(&app).and_then(|path| data_url(&path)),
    }
}

/// Read an image back as a `data:` URL, or `None` if it cannot be read.
fn data_url(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let media = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Some(format!("data:{media};base64,{}", base64(&bytes)))
}

/// Base64, written out here rather than taken as a dependency for one use.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |bits, (index, byte)| bits | (*byte as u32) << (16 - 8 * index));
        for index in 0..=chunk.len() {
            out.push(ALPHABET[(bits >> (18 - 6 * index) & 0b11_1111) as usize] as char);
        }
        for _ in chunk.len()..3 {
            out.push('=');
        }
    }
    out
}

/// Change the name published alongside this user's entries.
///
/// An empty name falls back to the machine's rather than being stored: a
/// nameless author would show as a blank beside an entry.
#[tauri::command]
pub fn set_my_name(app: AppHandle, state: State<AppState>, name: String) -> MyProfile {
    let name = name.trim();
    let name = if name.is_empty() {
        authors::name_from_the_machine()
    } else {
        name.to_owned()
    };
    save_settings(&app, |settings| settings.author_name = Some(name));
    republish(&app, &state);
    my_profile(app)
}

/// Set the user's picture from bytes the page read, returning the new profile.
///
/// Bytes rather than a path because the page has already loaded the image to
/// show it, and because this is the one route that works for a pasted picture
/// as well as a chosen file.
#[tauri::command]
pub fn set_my_avatar(
    app: AppHandle,
    state: State<AppState>,
    filename: String,
    bytes: Vec<u8>,
) -> CmdResult<MyProfile> {
    keep_avatar(&app, &filename, &bytes)?;
    republish(&app, &state);
    Ok(my_profile(app))
}

/// Make `bytes` the user's picture, named after `filename`'s extension.
fn keep_avatar(app: &AppHandle, filename: &str, bytes: &[u8]) -> Result<()> {
    let directory = app
        .path()
        .app_config_dir()
        .context("finding the app's config folder")?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("creating {}", directory.display()))?;

    // One fixed stem, so changing the picture replaces the old one instead of
    // leaving a folder of every picture ever chosen. The extension follows the
    // source, because it is what says how to decode it.
    let extension = Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| is_image(&format!("x.{extension}")))
        .unwrap_or("png")
        .to_ascii_lowercase();
    let name = format!("avatar.{extension}");
    atomic::write(&directory.join(&name), bytes).with_context(|| format!("writing {name}"))?;

    update_settings(app, |settings| {
        // An old picture with a different extension would otherwise sit there
        // unreferenced for good.
        if let Some(previous) = settings.author_avatar.take().filter(|previous| previous != &name) {
            let _ = fs::remove_file(directory.join(previous));
        }
        settings.author_avatar = Some(name);
    })
}

#[tauri::command]
pub fn clear_my_avatar(app: AppHandle, state: State<AppState>) -> MyProfile {
    let directory = app.path().app_config_dir();
    save_settings(&app, |settings| {
        if let (Some(name), Ok(directory)) = (settings.author_avatar.take(), directory) {
            let _ = fs::remove_file(directory.join(name));
        }
    });
    republish(&app, &state);
    my_profile(app)
}

/// Authors in the open project that this device might be.
///
/// Nothing carries an author id from one machine to the next — there is no
/// account for it to travel in — so a laptop and a phone would otherwise be two
/// people, and a journal kept by one person would start showing names. Instead,
/// a device that opens a project where it is not yet an author is asked which
/// of the ones there it is.
///
/// Empty when there is nothing to ask: this device already writes here, or
/// every author here is one it has been told is someone else. Aliases are left
/// out, being somebody already asked about under another id.
#[tauri::command]
pub fn unclaimed_authors(app: AppHandle, state: State<AppState>) -> Vec<String> {
    let settings = read_settings(&app);
    let Ok(root) = with_project(&app, &state, |open| Ok(root_of(open))) else {
        return Vec::new();
    };
    let here = authors::read_all(&root);
    if settings
        .author_id
        .as_ref()
        .is_some_and(|mine| here.contains_key(mine))
    {
        return Vec::new();
    }
    let mut unclaimed: Vec<String> = here
        .into_iter()
        .filter(|(id, profile)| profile.same_as.is_none() && !settings.not_me.contains(id))
        .map(|(id, _)| id)
        .collect();
    unclaimed.sort();
    unclaimed
}

/// Settle who this device is, from the authors [`unclaimed_authors`] offered.
///
/// `me` is the one chosen, or `None` for "none of these"; every other id in
/// `offered` is remembered as not this user, so they are not asked about again.
///
/// Claiming an author adopts it whole — its id, its name and its picture —
/// which is also how a new device gets a profile it was never given. An id this
/// device had already been writing under is not lost: its records in the
/// projects on the list are marked as the same person, so its entries are
/// counted as the claimed author's rather than a stranger's. The entries
/// themselves are never rewritten.
#[tauri::command]
pub fn claim_author(
    app: AppHandle,
    state: State<AppState>,
    me: Option<String>,
    offered: Vec<String>,
) -> CmdResult<MyProfile> {
    let root = with_project(&app, &state, |open| Ok(root_of(open)))?;
    let previous = read_settings(&app).author_id;

    let claimed = match &me {
        Some(id) => Some(
            authors::read_all(&root)
                .remove(id)
                .ok_or_else(|| anyhow!("there is no author {id} in this project"))?,
        ),
        None => None,
    };

    update_settings(&app, |settings| {
        for id in &offered {
            if Some(id) != me.as_ref() && !settings.not_me.contains(id) {
                settings.not_me.push(id.clone());
            }
        }
        if let (Some(id), Some(profile)) = (&me, &claimed) {
            settings.author_id = Some(id.clone());
            settings.author_name = Some(profile.name.clone());
        }
    })?;

    if let (Some(id), Some(profile)) = (&me, &claimed) {
        // The picture is copied rather than pointed at: the project may be
        // removed from this machine, and the profile is not the project's.
        if let Some(picture) = &profile.avatar {
            let source = root.join(authors::AUTHORS_DIR).join(id).join(picture);
            if let Ok(bytes) = fs::read(&source) {
                keep_avatar(&app, picture, &bytes)?;
            }
        }
        if let Some(previous) = previous.filter(|previous| previous != id) {
            let library = library::read(&app);
            for project in library.tabs.iter().flat_map(|tab| &tab.projects) {
                authors::mark_same_as(project, &previous, id);
            }
        }
    }
    Ok(my_profile(app))
}

/// What this user is called in the open project alone, when that differs.
#[tauri::command]
pub fn my_name_here(app: AppHandle, state: State<AppState>) -> Option<String> {
    let id = read_settings(&app).author_id?;
    with_project(&app, &state, |open| Ok(root_of(open)))
        .ok()
        .and_then(|root| authors::read_all(&root).remove(&id))
        .and_then(|profile| profile.display_name)
}

/// Set, or clear, what this user is called in the open project alone.
///
/// The record is theirs to write, which is what keeps the registry free of
/// conflicts, so this only ever touches their own folder — publishing it first
/// if they have not written here yet, since there is nothing to rename until
/// there is a record.
#[tauri::command]
pub fn set_my_name_here(
    app: AppHandle,
    state: State<AppState>,
    name: Option<String>,
) -> CmdResult<()> {
    let avatar = avatar_path(&app);
    Ok(with_project(&app, &state, |open| {
        let root = root_of(open);
        let (id, published) = author(&app).ok_or_else(|| anyhow!("there is no profile to rename"))?;
        authors::publish(&root, &id, &published, avatar.as_deref());
        authors::set_display_name(&root, &id, name.as_deref())
    })?)
}

/// Push a changed profile into the project on screen, if there is one.
///
/// Without this a rename would only reach a project the next time the user
/// wrote in it, so the name beside their own entries would stay the old one
/// while they were looking at it. Projects they are not in catch up when they
/// next write there.
fn republish(app: &AppHandle, state: &State<AppState>) {
    let avatar = avatar_path(app);
    let _ = with_project(app, state, |open| {
        let root = root_of(open);
        if let Some((id, name)) = author(app) {
            authors::publish(&root, &id, &name, avatar.as_deref());
        }
        Ok(())
    });
}

/// Who the user is — id and published name — minting and storing an identity
/// the first time it is asked for.
///
/// Returns `None` when the settings cannot be read or the identity cannot be
/// stored, in which case entries are written without an author rather than
/// with one that would be different on every launch. Minting is only ever
/// done from settings that were actually read: settings that could not be are
/// not "no identity yet", they are an identity this call cannot see, and a
/// fresh one written over it made one person look like a crowd.
fn author(app: &AppHandle) -> Option<(String, String)> {
    let _held = SETTINGS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut settings = load_settings(app).ok()?;
    if let (Some(id), Some(name)) = (&settings.author_id, &settings.author_name) {
        return Some((id.clone(), name.clone()));
    }

    let id = settings
        .author_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = settings
        .author_name
        .clone()
        .unwrap_or_else(authors::name_from_the_machine);
    settings.author_id = Some(id.clone());
    settings.author_name = Some(name.clone());
    // Only an identity that was actually stored is one the next launch will
    // agree with.
    write_settings(app, &settings).ok()?;
    Some((id, name))
}

/// The stored appearance, for whoever is building the window.
pub fn appearance_preference(app: &AppHandle) -> theme::Appearance {
    let settings = read_settings(app);
    theme::Appearance {
        choice: settings.theme,
        dark: settings.background_dark,
        light: settings.background_light,
    }
}

/// Remember the theme and its grounds, so the next launch opens a window of
/// the right colour.
///
/// Called by the page after it has already applied the theme to itself: nothing
/// on screen is waiting for this.
#[tauri::command]
pub fn set_theme_preference(
    app: AppHandle,
    theme: String,
    background_dark: String,
    background_light: String,
) {
    save_settings(&app, |settings| {
        settings.theme = Some(theme);
        settings.background_dark = Some(background_dark);
        settings.background_light = Some(background_light);
    });
}

/// Where new projects go when nothing has said otherwise.
///
/// In a development build that is the repo's own `projects/` folder, which is
/// where this project's journals are kept and committed alongside the code.
/// A shipped build has no repo, so it falls back to the user's documents.
fn built_in_projects_dir(app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        // `CARGO_MANIFEST_DIR` is `<repo>/src-tauri`, baked in at compile time.
        if let Some(repo) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
            return repo.join("projects");
        }
    }
    app.path()
        .document_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("Lazuli")
}

/// The folder the "new project" dialog should open in.
///
/// Created if it does not exist, so the dialog never opens on a missing path.
#[tauri::command]
pub fn default_projects_dir(app: AppHandle) -> PathBuf {
    let dir = read_settings(&app)
        .projects_dir
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| built_in_projects_dir(&app));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_config_dir()
        .map_err(|_| anyhow!("the app has no config folder"))
}

/// Held across every read-modify-write of the settings.
///
/// Commands run on more than one thread; two changes interleaved would end with
/// whichever wrote second having written from a copy that predates the first.
static SETTINGS: Mutex<()> = Mutex::new(());

/// A JSON file's contents, or `None` when there is no such file.
///
/// A file that is not there is a fresh install and means empty settings. A file
/// that is there and cannot be read or parsed is something else — a rename
/// mid-flight, another instance's handle, a bad byte — and is **not** empty
/// settings: reading it as empty is how a name, a picture and an identity were
/// once lost at a stroke, when the next writer took the emptiness for the truth.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("parsing {}", path.display()))
}

/// The settings kept in `dir`, or why they cannot be had.
fn settings_at(dir: &Path) -> Result<Settings> {
    let mut machine: MachineFile = read_json(&dir.join(SETTINGS_FILE))?.unwrap_or_default();
    let me: MeFile = match read_json(&dir.join(ME_FILE))? {
        Some(me) => me,
        // Before `profile.json` existed, the builds that had a profile kept it
        // in `settings.json`, where it arrives here as fields this struct does
        // not know. Taken from there once; the first save moves it for good.
        None => {
            let carried: serde_json::Map<String, serde_json::Value> = ME_FIELDS
                .iter()
                .filter_map(|field| Some((field.to_string(), machine.rest.get(*field)?.clone())))
                .collect();
            serde_json::from_value(serde_json::Value::Object(carried)).unwrap_or_default()
        }
    };
    // Wherever the profile came from, `settings.json` is not where it lives.
    for field in ME_FIELDS.iter().chain(&RETIRED_FIELDS) {
        machine.rest.remove(*field);
    }
    Ok(Settings {
        projects_dir: machine.projects_dir,
        theme: machine.theme,
        background_dark: machine.background_dark,
        background_light: machine.background_light,
        author_id: me.author_id,
        author_name: me.author_name,
        author_avatar: me.author_avatar,
        not_me: me.not_me,
        machine_rest: machine.rest,
        me_rest: me.rest,
    })
}

/// The settings, retrying briefly: on Windows a file that is being renamed
/// over is unopenable for the duration, and another instance of the app may
/// be doing exactly that.
fn load_settings(app: &AppHandle) -> Result<Settings> {
    let dir = config_dir(app)?;
    let mut attempts = 0;
    loop {
        match settings_at(&dir) {
            Err(err) if attempts < 5 => {
                attempts += 1;
                eprintln!("lazuli: settings unreadable, retrying: {err:#}");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => return result,
        }
    }
}

/// The settings for reading only. A failure reads as empty, which is harmless
/// here because nothing reads them this way and then writes them back.
fn read_settings(app: &AppHandle) -> Settings {
    load_settings(app).unwrap_or_default()
}

/// Change the settings on disk, or say why they could not be.
///
/// The only way to write them. Nothing is written when they could not be read
/// first, whatever the reason: a change the user asked for and lost is a
/// change they make again, where a file written from nothing is everything
/// else they had, gone.
fn update_settings(app: &AppHandle, change: impl FnOnce(&mut Settings)) -> Result<()> {
    let _held = SETTINGS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut settings = load_settings(app)?;
    change(&mut settings);
    write_settings(app, &settings)
}

/// [`update_settings`] for the callers that have nowhere to send an error:
/// it is logged, and the settings are left as they were.
fn save_settings(app: &AppHandle, change: impl FnOnce(&mut Settings)) {
    if let Err(err) = update_settings(app, change) {
        eprintln!("lazuli: settings not saved: {err:#}");
    }
}

fn write_settings(app: &AppHandle, settings: &Settings) -> Result<()> {
    write_settings_to(&config_dir(app)?, settings)
}

/// Write both files. The profile first: a run stopped between the two leaves a
/// `profile.json` that is complete and a `settings.json` that still carries the
/// old copy of it, and the profile file is the one that wins.
fn write_settings_to(dir: &Path, settings: &Settings) -> Result<()> {
    let _ = fs::create_dir_all(dir);
    let me = MeFile {
        author_id: settings.author_id.clone(),
        author_name: settings.author_name.clone(),
        author_avatar: settings.author_avatar.clone(),
        not_me: settings.not_me.clone(),
        rest: settings.me_rest.clone(),
    };
    let machine = MachineFile {
        projects_dir: settings.projects_dir.clone(),
        theme: settings.theme.clone(),
        background_dark: settings.background_dark.clone(),
        background_light: settings.background_light.clone(),
        rest: settings.machine_rest.clone(),
    };
    atomic::write(
        &dir.join(ME_FILE),
        serde_json::to_string_pretty(&me).context("serialising the profile")?,
    )?;
    atomic::write(
        &dir.join(SETTINGS_FILE),
        serde_json::to_string_pretty(&machine).context("serialising the settings")?,
    )
}

/// Remember where a project was created, so the next one is offered the same
/// place. Deliberately not updated on *open*: opening one project from
/// elsewhere should not move where new ones are made.
fn remember_projects_dir(app: &AppHandle, root: &Path) {
    let Some(parent) = root.parent() else {
        return;
    };
    let parent = parent.to_path_buf();
    save_settings(app, |settings| settings.projects_dir = Some(parent));
}

/// A project's metadata without opening it, for the launch screen.
#[tauri::command]
pub fn peek_project(path: PathBuf) -> CmdResult<ProjectMeta> {
    Ok(store::read_meta(&path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-rolled, so checked against the vectors in RFC 4648 — including the
    /// two padded lengths, which are what an off-by-one gets wrong.
    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_covers_the_whole_byte_range() {
        // The high bytes of a PNG are where a sign error would show up.
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64(&[0x00, 0x00, 0x00]), "AAAA");
    }

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lazuli-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("a temp dir");
        dir
    }

    #[test]
    fn no_settings_file_is_empty_settings() {
        let dir = scratch("settings-missing");
        let settings = settings_at(&dir).expect("a fresh install reads");
        assert!(settings.author_id.is_none());
        assert!(settings.not_me.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_settings_file_that_will_not_parse_is_not_empty_settings() {
        // A file present but unreadable once came back as defaults, the next
        // writer persisted them, and the name and the picture were gone
        // together. Broken must stay an error, so nothing downstream takes it
        // for a blank slate — and that holds for either file.
        let dir = scratch("settings-broken");
        fs::write(dir.join(SETTINGS_FILE), "{ \"theme\": \"da").unwrap();
        assert!(settings_at(&dir).is_err());

        let dir2 = scratch("profile-broken");
        fs::write(dir2.join(ME_FILE), "{ \"author_id\": \"a1b2").unwrap();
        assert!(settings_at(&dir2).is_err());
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(dir2);
    }

    #[test]
    fn a_rewrite_by_the_0_4_release_cannot_take_the_profile_with_it() {
        // The loss this split exists for: 0.4.0 reads `settings.json` into four
        // fields and writes those four back. Simulate exactly that write, over
        // a machine that had a profile, and the profile must still be there.
        let dir = scratch("settings-release");
        let mut settings = Settings {
            theme: Some("dark".into()),
            author_id: Some("me".into()),
            author_name: Some("Jules".into()),
            author_avatar: Some("avatar.png".into()),
            ..Settings::default()
        };
        write_settings_to(&dir, &settings).unwrap();

        fs::write(
            dir.join(SETTINGS_FILE),
            r#"{ "projects_dir": null, "theme": "light", "background_dark": null, "background_light": null }"#,
        )
        .unwrap();

        settings = settings_at(&dir).unwrap();
        assert_eq!(settings.theme.as_deref(), Some("light"), "the release's own change stands");
        assert_eq!(settings.author_id.as_deref(), Some("me"));
        assert_eq!(settings.author_name.as_deref(), Some("Jules"));
        assert_eq!(settings.author_avatar.as_deref(), Some("avatar.png"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_field_this_build_does_not_know_survives_a_rewrite() {
        // So the next build that falls behind another cannot do what 0.4.0 did.
        let dir = scratch("settings-unknown");
        fs::write(
            dir.join(SETTINGS_FILE),
            r#"{ "theme": "dark", "from_the_future": [1, 2] }"#,
        )
        .unwrap();
        fs::write(dir.join(ME_FILE), r#"{ "author_id": "me", "pronouns": "they" }"#).unwrap();

        let mut settings = settings_at(&dir).unwrap();
        settings.theme = Some("light".into());
        write_settings_to(&dir, &settings).unwrap();

        let machine: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap()).unwrap();
        let me: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join(ME_FILE)).unwrap()).unwrap();
        assert_eq!(machine["from_the_future"], serde_json::json!([1, 2]));
        assert_eq!(machine["theme"], "light");
        assert_eq!(me["pronouns"], "they");
        assert_eq!(me["author_id"], "me");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_profile_kept_in_settings_json_by_an_earlier_build_moves_out() {
        // The dev builds before the split wrote the profile into
        // `settings.json`. It is read from there once and saved where it
        // belongs, and does not stay behind as a second copy.
        let dir = scratch("settings-migrate");
        fs::write(
            dir.join(SETTINGS_FILE),
            r#"{ "theme": "dark", "author_id": "me", "author_name": "Jules", "not_me": ["ana"] }"#,
        )
        .unwrap();

        let settings = settings_at(&dir).unwrap();
        assert_eq!(settings.author_id.as_deref(), Some("me"));
        assert_eq!(settings.not_me, ["ana"]);
        write_settings_to(&dir, &settings).unwrap();

        let machine = fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(!machine.contains("author_id"), "the profile left settings.json");
        let again = settings_at(&dir).unwrap();
        assert_eq!(again.author_name.as_deref(), Some("Jules"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_google_sign_in_of_the_drive_builds_is_dropped() {
        // Unknown fields are carried through a rewrite; a refresh token that
        // nothing uses is deliberately not.
        let dir = scratch("settings-drive");
        fs::write(
            dir.join(SETTINGS_FILE),
            r#"{ "author_id": "me", "drive_tokens": { "refresh_token": "x" }, "drive_account": "google:1" }"#,
        )
        .unwrap();
        let settings = settings_at(&dir).expect("an old file reads");
        assert_eq!(settings.author_id.as_deref(), Some("me"));
        write_settings_to(&dir, &settings).unwrap();
        let machine = fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(!machine.contains("drive_tokens"));
        assert!(!machine.contains("refresh_token"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_settings_file_from_an_older_build_still_parses() {
        // Every field is optional, so a file that predates the newest of them
        // is a fresh install for that field and nothing else.
        let dir = scratch("settings-old");
        fs::write(dir.join(SETTINGS_FILE), "{ \"theme\": \"dark\" }").unwrap();
        let settings = settings_at(&dir).expect("an old file reads");
        assert_eq!(settings.theme.as_deref(), Some("dark"));
        assert!(settings.not_me.is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
