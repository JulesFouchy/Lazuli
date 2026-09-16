//! The Tauri command surface, and the state behind it.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Local, NaiveDate};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::dates;
use crate::library::{self, Slot};
use crate::model::{is_image, DateFormat, Project, ProjectMeta, SortOrder};
use crate::paths::{folder_name_for, is_named_after, unique_path};
use crate::store::{self, ProjectStore};
use crate::theme;
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
    /// Where the file or folder was, before it went to the Recycle Bin.
    original_path: PathBuf,
    /// The entry whose `image:` field was cleared by the delete, if any, so
    /// undo restores the choice and not merely the file.
    cleared_choice_for: Option<String>,
    /// Set when the delete cleared the project's chosen cover.
    cleared_cover: bool,
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

    // Without this the webview silently refuses to load any image in the
    // folder: `file://` is blocked, and `convertFileSrc` only works for paths
    // the asset scope allows.
    app.asset_protocol_scope()
        .allow_directory(&path, true)
        .with_context(|| format!("allowing asset access to {}", path.display()))?;

    let mut store = ProjectStore::new(&path);
    let snapshot = store.scan()?;
    let watcher = watch::watch_project(app.clone(), &path)?;

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
    Ok(with_project(&app, &state, |open| {
        store::create_entry(&root_of(open), date, Local::now().fixed_offset())
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
        fs::write(&destination, &bytes)
            .with_context(|| format!("writing {}", destination.display()))?;
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

/// Send one image to the Recycle Bin.
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

        let mut record = Trashed {
            original_path: path.clone(),
            cleared_choice_for: None,
            cleared_cover: false,
        };

        match entry_id.as_deref() {
            Some(id) => {
                let entry_file = dir.join(store::ENTRY_FILE);
                let (frontmatter, _) = store::read_entry_file(&entry_file)?;
                if frontmatter.image.as_deref() == Some(filename.as_str()) {
                    store::update_entry(&root, id, None, None, Some(None))?;
                    record.cleared_choice_for = Some(id.to_owned());
                }
            }
            None => {
                let mut meta = store::read_meta(&root)?;
                if meta.cover.as_deref() == Some(filename.as_str()) {
                    meta.cover = None;
                    store::write_meta(&root, &meta)?;
                    record.cleared_cover = true;
                }
            }
        }

        trash_with_retry(&path)?;
        push_undo(open, record);
        Ok(())
    })?;
    Ok(())
}

/// Send a whole entry folder, images and all, to the Recycle Bin.
#[tauri::command]
pub fn trash_entry(app: AppHandle, state: State<AppState>, id: String) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let dir = store::entry_dir(&root_of(open), &id);
        if !dir.is_dir() {
            bail!("no entry {id} in this project");
        }
        trash_with_retry(&dir)?;
        push_undo(
            open,
            Trashed {
                original_path: dir,
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
        let message = match restore(&record.original_path) {
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

                let original = file_name_of(&record.original_path);
                if restored_as == original {
                    format!("Restored {restored_as}")
                } else {
                    // The old name was taken in the meantime; say so rather
                    // than letting a file appear under a name nobody chose.
                    format!("Restored as {restored_as}")
                }
            }
            // The Recycle Bin was emptied, or the item is otherwise gone. The
            // record is already popped, so a second Ctrl+Z moves further back.
            Err(err) => format!("Could not undo: {err:#}"),
        };
        Ok(Some(UndoOutcome {
            message,
            more: !open.undo.is_empty(),
        }))
    })?)
}

/// How long a Recycle Bin move keeps retrying, and the gap between tries.
///
/// Long enough to ride out something letting go of the folder, short enough
/// that a folder held for good reports it promptly: the row is already gone
/// from the screen and comes back when this gives up, so a long window would
/// mean a long silence before the row reappeared.
const TRASH_RETRY_WINDOW: Duration = Duration::from_secs(2);
const TRASH_RETRY_STEP: Duration = Duration::from_millis(150);

/// Run blocking filesystem work away from the main thread.
///
/// A `#[tauri::command] fn` runs on the main thread, where a Recycle Bin move
/// taking a second or two, or a scan of a large project, blocks every other
/// command behind it and freezes the page meanwhile. Deleting one project used
/// to empty the whole launch list until the move finished, because the
/// `recent_projects` call that redraws it was stuck in that queue.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .context("a filesystem task did not finish")?
}

/// Move a path to the Recycle Bin, retrying while something still holds it.
///
/// The shell will not move a folder anything has an open handle on, and reports
/// that as "some operations were aborted" rather than as a lock, so the message
/// says nothing useful on its own.
///
/// Our own watcher is not the holder: its handle is opened with delete sharing
/// and a watched project moves fine. The holders are outside the app — editors,
/// search indexers, file watchers in other tools (the Vite dev server was one,
/// until `vite.config.ts` told it to ignore `projects/`) — and nothing here can
/// make them let go. Some release within a moment, hence a window rather than
/// a fixed number of tries; the rest get a message that says what to do.
fn trash_with_retry(path: &Path) -> Result<()> {
    let deadline = Instant::now() + TRASH_RETRY_WINDOW;
    loop {
        let Err(err) = trash::delete(path) else {
            return Ok(());
        };
        if Instant::now() >= deadline {
            return Err(anyhow!(err)).with_context(|| {
                format!(
                    "{} is still open in another program, so it cannot be moved \
                     to the Recycle Bin. Close anything using it and try again.",
                    path.display()
                )
            });
        }
        std::thread::sleep(TRASH_RETRY_STEP);
    }
}

/// Pull one item back out of the Recycle Bin, returning the name it landed
/// under.
///
/// `restore_all` can only restore to the original path, so when that path is
/// occupied the occupant is moved aside, the restore happens, the *restored*
/// file takes the ` (2)` suffix, and the occupant goes back to its own name.
/// The suffix goes to the restored file deliberately: the occupant is the file
/// the user just put there, may already be referenced as a chosen image, and
/// should not change name under them.
///
/// Windows and Linux only. `trash::os_limited` — the half of the crate that can
/// read the bin back — does not exist on macOS, because macOS offers no API for
/// it: the Finder's own "Put Back" reads a private file nothing else may touch.
/// See the macOS arm below for what happens there instead.
#[cfg(not(target_os = "macos"))]
fn restore(original_path: &Path) -> Result<String> {
    let parent = original_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent folder", original_path.display()))?;
    let name = original_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("{} has no filename", original_path.display()))?;

    let item = trash::os_limited::list()
        .context("listing the Recycle Bin")?
        .into_iter()
        .filter(|item| item.original_parent == parent && item.name == name)
        // Several deletions of the same name can be in the bin; the most
        // recent is the one this undo refers to.
        .max_by_key(|item| item.time_deleted)
        .ok_or_else(|| anyhow!("{name} is no longer in the Recycle Bin"))?;

    if !original_path.exists() {
        trash::os_limited::restore_all([item]).context("restoring from the Recycle Bin")?;
        return Ok(name.to_owned());
    }

    let stash = unique_path(parent, &format!("{name}.lazuli-restoring"));
    fs::rename(original_path, &stash).with_context(|| {
        format!(
            "moving {} aside to make room for the restore",
            original_path.display()
        )
    })?;

    let outcome = (|| -> Result<String> {
        trash::os_limited::restore_all([item]).context("restoring from the Recycle Bin")?;
        let renamed = unique_path(parent, name);
        fs::rename(original_path, &renamed).with_context(|| {
            format!("renaming the restored file to {}", renamed.display())
        })?;
        Ok(file_name_of(&renamed))
    })();

    // Put the occupant back under its own name whether or not the restore
    // worked; leaving it stashed would be worse than the failed undo.
    fs::rename(&stash, original_path).with_context(|| {
        format!("restoring {} to its own name", original_path.display())
    })?;
    outcome
}

/// What undoing a delete does on macOS, where it cannot be done.
///
/// Nothing is lost — the entry or image is in the Trash and Finder's "Put Back"
/// will return it — but the app cannot do it, so the offer has to be withdrawn
/// honestly rather than failing with something about a missing item. The toast
/// that carries this is the same one that offered the undo.
///
/// The way out is not a macOS restore API; there is none. It is to stop using
/// the system trash for this and keep a trash folder inside the project, which
/// would behave the same on all three platforms — see
/// `ideas/portable-undo-delete.md`.
#[cfg(target_os = "macos")]
fn restore(original_path: &Path) -> Result<String> {
    let name = original_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("it");
    bail!(
        "Lazuli cannot take {name} back out of the Trash on macOS.          It is still there — open the Trash and use Put Back."
    )
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
    library::read(app)
        .tabs
        .into_iter()
        .map(|tab| TabView {
            name: tab.name,
            projects: tab
                .projects
                .into_iter()
                // A folder that is no longer a project is skipped rather than
                // dropped: the file is the user's arrangement, and a disk that
                // is not mounted this morning is not a reason to rewrite it.
                .filter(|path| store::is_project(path))
                .map(|path| listed_project(app, path))
                .collect(),
        })
        .collect()
}

fn listed_project(app: &AppHandle, path: PathBuf) -> ListedProject {
    let meta = store::read_meta(&path).ok();
    // The launch screen shows each project behind its own cover, so the
    // webview has to be allowed to load that one folder. Not recursive,
    // and not the whole project: nothing else is shown until it opens.
    if meta.as_ref().is_some_and(|meta| meta.cover.is_some()) {
        let _ = app
            .asset_protocol_scope()
            .allow_directory(path.join(store::COVER_DIR), false);
    }
    ListedProject {
        name: meta
            .as_ref()
            .map(|meta| meta.name.clone())
            .unwrap_or_else(|| file_name_of(&path)),
        cover: meta.and_then(|meta| meta.cover),
        path,
    }
}

#[derive(Debug, Serialize)]
pub struct TabView {
    pub name: String,
    pub projects: Vec<ListedProject>,
}

#[derive(Debug, Serialize)]
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

/// Move a whole project folder to the Recycle Bin.
///
/// Returns its slot in the list, so the undo can put both the folder and the
/// listing back.
#[tauri::command]
pub async fn trash_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> CmdResult<Option<Slot>> {
    if !store::is_project(&path) {
        return Err(anyhow!("{} is not a Lazuli project", path.display()).into());
    }

    // Closing first so no rescan runs against a folder that is on its way to
    // the Recycle Bin. The watcher's own handle does not block the move.
    if state
        .open
        .lock()
        .expect("project lock was poisoned")
        .as_ref()
        .is_some_and(|open| open.store.root() == path)
    {
        close_open(&app);
    }

    let trashing = path.clone();
    off_thread(move || trash_with_retry(&trashing)).await?;
    Ok(forget_project(app, path))
}

/// Take a deleted project back out of the Recycle Bin and back into the list.
#[tauri::command]
pub async fn restore_project(
    app: AppHandle,
    path: PathBuf,
    slot: Slot,
) -> CmdResult<PathBuf> {
    let restoring = path.clone();
    let restored_as = off_thread(move || restore(&restoring)).await?;
    // The old name may have been taken in the meantime, in which case the
    // folder comes back under a different one and the list must follow it.
    let actual = path.with_file_name(restored_as);
    restore_listing(app, actual.clone(), slot);
    Ok(actual)
}

const SETTINGS_FILE: &str = "settings.json";

/// App-level settings, kept next to the project list rather than in any
/// project folder.
#[derive(Debug, Default, Serialize, serde::Deserialize)]
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
    let mut settings = read_settings(&app);
    settings.theme = Some(theme);
    settings.background_dark = Some(background_dark);
    settings.background_light = Some(background_light);
    write_settings(&app, &settings);
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

fn settings_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(SETTINGS_FILE))
}

fn read_settings(app: &AppHandle) -> Settings {
    settings_file(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_settings(app: &AppHandle, settings: &Settings) {
    let Some(path) = settings_file(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, text);
    }
}

/// Remember where a project was created, so the next one is offered the same
/// place. Deliberately not updated on *open*: opening one project from
/// elsewhere should not move where new ones are made.
fn remember_projects_dir(app: &AppHandle, root: &Path) {
    let Some(parent) = root.parent() else {
        return;
    };
    // Read-modify-write rather than writing a fresh `Settings`: the file holds
    // more than this field, and building one here would silently drop the rest.
    let mut settings = read_settings(app);
    settings.projects_dir = Some(parent.to_path_buf());
    write_settings(app, &settings);
}

/// A project's metadata without opening it, for the launch screen.
#[tauri::command]
pub fn peek_project(path: PathBuf) -> CmdResult<ProjectMeta> {
    Ok(store::read_meta(&path)?)
}
