//! The Tauri command surface, and the state behind it.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDate};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::dates;
use crate::model::{is_image, Project, ProjectMeta};
use crate::paths::unique_path;
use crate::store::{self, ProjectStore};
use crate::video::{self, Encode, ExportOptions};
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
    /// Held so the watcher stays alive; dropped when the project closes.
    _watcher: ProjectWatcher,
    undo: Vec<Trashed>,
}

#[derive(Default)]
pub struct AppState {
    open: Mutex<Option<OpenProject>>,
    /// The export in flight, if any. Frames are pushed into it one at a time.
    export: Mutex<Option<Encode>>,
}

/// Cap on the undo stack. Session-only anyway; this just stops a long tidying
/// session from growing it without bound.
const MAX_UNDO: usize = 50;

// --- opening and closing -------------------------------------------------

#[tauri::command]
pub fn open_project(app: AppHandle, state: State<AppState>, path: PathBuf) -> CmdResult<Project> {
    Ok(open_at(&app, &state, path)?)
}

#[tauri::command]
pub fn create_project(
    app: AppHandle,
    state: State<AppState>,
    path: PathBuf,
    name: String,
) -> CmdResult<Project> {
    store::create_project(&path, &name, dates::today())?;
    remember_projects_dir(&app, &path);
    Ok(open_at(&app, &state, path)?)
}

fn open_at(app: &AppHandle, state: &State<AppState>, path: PathBuf) -> Result<Project> {
    if !store::is_project(&path) {
        bail!(
            "{} is not a Journaley project (no {} inside)",
            path.display(),
            store::META_FILE
        );
    }

    // Without this the webview silently refuses to load any image in the
    // folder: `file://` is blocked, and `convertFileSrc` only works for paths
    // the asset scope allows.
    app.asset_protocol_scope()
        .allow_directory(&path, true)
        .with_context(|| format!("allowing asset access to {}", path.display()))?;

    let mut store = ProjectStore::new(&path);
    let snapshot = store.scan()?;
    let watcher = watch::watch_project(app.clone(), &path)?;

    remember_recent(app, &path);
    *state.open.lock().expect("project lock was poisoned") = Some(OpenProject {
        store,
        snapshot: snapshot.clone(),
        _watcher: watcher,
        undo: Vec::new(),
    });
    Ok(snapshot)
}

#[tauri::command]
pub fn close_project(state: State<AppState>) {
    *state.open.lock().expect("project lock was poisoned") = None;
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
        Err(err) => eprintln!("journaley: rescan failed: {err:#}"),
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

#[tauri::command]
pub fn set_project_name(app: AppHandle, state: State<AppState>, name: String) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        let root = root_of(open);
        let mut meta = store::read_meta(&root)?;
        meta.name = name;
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

#[tauri::command]
pub fn create_entry(
    app: AppHandle,
    state: State<AppState>,
    created: Option<DateTime<FixedOffset>>,
) -> CmdResult<String> {
    let created = created.unwrap_or_else(|| Local::now().fixed_offset());
    Ok(with_project(&app, &state, |open| {
        store::create_entry(&root_of(open), created)
    })?)
}

#[tauri::command]
pub fn update_entry(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    created: Option<DateTime<FixedOffset>>,
    text: Option<String>,
    // `Some(None)` clears the chosen image; `None` leaves it alone. Serde maps
    // an absent field to the outer `None` and an explicit `null` to the inner.
    image: Option<Option<String>>,
) -> CmdResult<()> {
    with_project(&app, &state, |open| {
        store::update_entry(
            &root_of(open),
            &id,
            created,
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
        Ok(named)
    })?)
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
        if !is_image(&filename) {
            bail!("{filename} is not a recognised image type");
        }
        let target = target_dir(open, entry_id.as_deref())?;
        fs::create_dir_all(&target)
            .with_context(|| format!("creating {}", target.display()))?;
        let destination = unique_path(&target, &filename);
        fs::write(&destination, &bytes)
            .with_context(|| format!("writing {}", destination.display()))?;
        Ok(file_name_of(&destination))
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

        trash::delete(&path).with_context(|| format!("moving {} to the Recycle Bin", path.display()))?;
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
        trash::delete(&dir)
            .with_context(|| format!("moving {} to the Recycle Bin", dir.display()))?;
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

/// Pull one item back out of the Recycle Bin, returning the name it landed
/// under.
///
/// `restore_all` can only restore to the original path, so when that path is
/// occupied the occupant is moved aside, the restore happens, the *restored*
/// file takes the ` (2)` suffix, and the occupant goes back to its own name.
/// The suffix goes to the restored file deliberately: the occupant is the file
/// the user just put there, may already be referenced as a chosen image, and
/// should not change name under them.
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

    let stash = unique_path(parent, &format!("{name}.journaley-restoring"));
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

// --- video export ---------------------------------------------------------

/// Where ffmpeg was found, so the UI can say what is wrong before the user
/// spends time rendering frames.
#[tauri::command]
pub fn ffmpeg_status(app: AppHandle) -> Option<String> {
    let resources = app.path().resource_dir().ok();
    video::find_ffmpeg(resources.as_deref())
        .map(|(path, _)| path.display().to_string())
}

/// Start ffmpeg and leave it waiting for frames.
#[tauri::command]
pub fn export_begin(app: AppHandle, state: State<AppState>, options: ExportOptions) -> CmdResult<()> {
    let resources = app.path().resource_dir().ok();
    let (ffmpeg, _) = video::find_ffmpeg(resources.as_deref()).ok_or_else(|| {
        anyhow!(
            "ffmpeg was not found. Put an ffmpeg executable in the app folder,              or install one on your PATH."
        )
    })?;

    let mut slot = state.export.lock().expect("export lock was poisoned");
    if slot.is_some() {
        return Err(anyhow!("an export is already running").into());
    }
    *slot = Some(Encode::start(&ffmpeg, &options)?);
    Ok(())
}

/// Hand ffmpeg the next frame. Blocks while ffmpeg is busy, which is what
/// paces the frontend's render loop and keeps memory flat.
#[tauri::command]
pub fn export_push_frame(state: State<AppState>, png: Vec<u8>) -> CmdResult<u32> {
    let mut slot = state.export.lock().expect("export lock was poisoned");
    let encode = slot
        .as_mut()
        .ok_or_else(|| anyhow!("no export is running"))?;
    match encode.push_frame(&png) {
        Ok(()) => Ok(encode.frames_written),
        Err(err) => {
            // ffmpeg has died; tear the export down rather than leaving a
            // wedged encoder for the next attempt to trip over.
            if let Some(encode) = slot.take() {
                encode.cancel();
            }
            Err(err.into())
        }
    }
}

/// Close the stream and wait for the file to be written.
#[tauri::command]
pub fn export_finish(state: State<AppState>) -> CmdResult<PathBuf> {
    let encode = state
        .export
        .lock()
        .expect("export lock was poisoned")
        .take()
        .ok_or_else(|| anyhow!("no export is running"))?;
    Ok(encode.finish()?)
}

/// Abandon the export and delete the half-written file.
#[tauri::command]
pub fn export_cancel(state: State<AppState>) {
    if let Some(encode) = state
        .export
        .lock()
        .expect("export lock was poisoned")
        .take()
    {
        encode.cancel();
    }
}

// --- misc ----------------------------------------------------------------

/// A project folder named on the command line, so `journaley <folder>` opens
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

/// The list of recently opened project folders.
///
/// The one piece of state that is not in a project folder, because it is about
/// the app rather than any one project.
#[tauri::command]
pub fn recent_projects(app: AppHandle) -> Vec<RecentProject> {
    read_recent(&app)
        .into_iter()
        .filter(|path| store::is_project(path))
        .map(|path| RecentProject {
            name: store::read_meta(&path)
                .map(|meta| meta.name)
                .unwrap_or_else(|_| file_name_of(&path)),
            path,
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct RecentProject {
    pub name: String,
    pub path: PathBuf,
}

#[tauri::command]
pub fn forget_recent(app: AppHandle, path: PathBuf) {
    let remaining: Vec<PathBuf> = read_recent(&app)
        .into_iter()
        .filter(|candidate| candidate != &path)
        .collect();
    write_recent(&app, &remaining);
}

const RECENT_FILE: &str = "recent.json";
const SETTINGS_FILE: &str = "settings.json";
const MAX_RECENT: usize = 12;

/// App-level settings, kept next to the recents list rather than in any
/// project folder.
#[derive(Debug, Default, Serialize, serde::Deserialize)]
struct Settings {
    /// Where the "new project" dialog opens. Updated whenever a project is
    /// created somewhere else, so the app follows wherever you keep them.
    projects_dir: Option<PathBuf>,
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
        .join("Journaley")
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
    write_settings(
        app,
        &Settings {
            projects_dir: Some(parent.to_path_buf()),
        },
    );
}

fn recent_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(RECENT_FILE))
}

fn read_recent(app: &AppHandle) -> Vec<PathBuf> {
    let Some(path) = recent_path(app) else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_recent(app: &AppHandle, paths: &[PathBuf]) {
    let Some(path) = recent_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(paths) {
        let _ = fs::write(path, text);
    }
}

fn remember_recent(app: &AppHandle, root: &Path) {
    let mut paths = read_recent(app);
    paths.retain(|candidate| candidate != root);
    paths.insert(0, root.to_path_buf());
    paths.truncate(MAX_RECENT);
    write_recent(app, &paths);
}

/// A project's metadata without opening it, for the launch screen.
#[tauri::command]
pub fn peek_project(path: PathBuf) -> CmdResult<ProjectMeta> {
    Ok(store::read_meta(&path)?)
}
