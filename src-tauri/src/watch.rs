//! Filesystem watching.
//!
//! The watcher never says *what* changed. It only says *something* changed, and
//! the project is re-read and diffed. That is what makes the app's own writes
//! silent: saving `entry.md` fires an event, the rescan matches what is already
//! in memory, and nothing is emitted, so the frontend never clobbers a textarea
//! being typed into.

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use std::path::Path;
use std::time::Duration;
use tauri::AppHandle;

/// How long the folder must be quiet before a batch is delivered.
///
/// Dropping a dozen images produces a storm of create and modify events;
/// without this they would each trigger a rescan.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// The watcher handle. Dropping it stops watching.
pub type ProjectWatcher = Debouncer<notify::RecommendedWatcher, RecommendedCache>;

/// Start watching a project folder, rescanning on every debounced batch.
pub fn watch_project(app: AppHandle, root: &Path) -> Result<ProjectWatcher> {
    let mut debouncer = new_debouncer(
        DEBOUNCE,
        None,
        move |result: DebounceEventResult| match result {
            // The events themselves are deliberately ignored: see the module
            // docs. Any batch at all means "look at the disk again".
            Ok(_) => crate::commands::rescan_and_emit(&app),
            Err(errors) => {
                for error in errors {
                    eprintln!("lapis: watch error: {error}");
                }
            }
        },
    )
    .context("starting the filesystem watcher")?;

    debouncer
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", root.display()))?;
    Ok(debouncer)
}
