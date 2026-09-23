//! The commands that turn syncing on, and the thread that keeps it running.
//!
//! Kept apart from `commands.rs` because it is the one part of the app that
//! reaches the network, and from `sync.rs` because that knows nothing about
//! Tauri, an account, or when to run — which is what makes it testable without
//! either.

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

use crate::drive;
use crate::store;
use crate::sync;

/// How often a synced project is reconciled while it is open.
///
/// A journal is not a chat: a change that reaches the other machine within the
/// minute is as good as one that reaches it instantly, and a quiet app should
/// not be talking to Google every few seconds.
const EVERY: Duration = Duration::from_secs(60);

/// The account, as the UI shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub connected: bool,
    /// What to say when it cannot be connected at all, or `None` when it can.
    pub unavailable: Option<String>,
}

/// Whether this build can sign in to Google.
fn why_not() -> Option<String> {
    (drive::DESKTOP_CLIENT_ID.is_empty() || drive::DESKTOP_CLIENT_SECRET.is_empty()).then(|| {
        "This build has no Google client id and secret, so it cannot sign in to \
         Drive. See the README."
            .to_owned()
    })
}

#[tauri::command]
pub fn drive_account(app: AppHandle) -> Account {
    Account {
        connected: crate::commands::drive_tokens(&app)
            .is_some_and(|tokens| !tokens.refresh_token.is_empty()),
        unavailable: why_not(),
    }
}

/// Sign in to Google, and remember the account.
///
/// Runs off the main thread and blocks for as long as the user is on Google's
/// page: the loopback listener is what the sign-in comes back to.
#[tauri::command]
pub async fn connect_drive(app: AppHandle) -> Result<Account, crate::commands::CmdError> {
    if let Some(why) = why_not() {
        return Err(anyhow!(why).into());
    }
    let tokens = crate::commands::off_thread(move || {
        let pkce = drive::Pkce::new();
        // The port is picked by the OS, so the URL Google is told to come back
        // to is only known here — and the exchange has to repeat exactly it.
        let redirect = drive::Redirect::new()?;
        let url = redirect.url.clone();
        tauri_plugin_opener::open_url(drive::sign_in_url(&pkce, &url), None::<&str>)
            .context("opening the Google sign-in page")?;
        let code = redirect.wait_for_code()?;
        drive::exchange(&code, &pkce, &url)
    })
    .await?;
    crate::commands::set_drive_tokens(&app, Some(tokens.clone()));

    // Asked once, here, because it is what a second device looks itself up by
    // in a project's `authors/` — and a round trip to Google before every entry
    // would be absurd. A failure is not worth undoing a good sign-in for; the
    // next thing needing a token asks again.
    remember_account(&app, &tokens.access_token);
    Ok(drive_account(app))
}

#[tauri::command]
pub fn disconnect_drive(app: AppHandle) -> Account {
    crate::commands::set_drive_tokens(&app, None);
    crate::commands::set_drive_account_id(&app, None);
    Account {
        connected: false,
        unavailable: why_not(),
    }
}

/// Whether the open project syncs, and what happened last time it tried.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub on: bool,
    /// The last failure, kept so a sync that is quietly not working says so.
    pub problem: Option<String>,
}

/// An access token good for right now, refreshing it if it is close to expiry.
fn token(app: &AppHandle) -> Result<String> {
    let tokens = crate::commands::drive_tokens(app)
        .ok_or_else(|| anyhow!("no Google account is connected"))?;
    let access = if tokens.is_fresh(std::time::SystemTime::now()) {
        tokens.access_token
    } else {
        let refreshed = drive::refresh(&tokens)?;
        let access = refreshed.access_token.clone();
        crate::commands::set_drive_tokens(app, Some(refreshed));
        access
    };
    remember_account(app, &access);
    Ok(access)
}

/// Learn which account these tokens belong to, if that is not already known.
///
/// Connecting records it, but an account connected by a build from before that
/// existed has tokens and no account — and without one a second device cannot
/// recognise its own author record and writes as a stranger. Asked here because
/// this is the one place with a token in hand and a network to use it on, and
/// it costs a single request, once, ever.
fn remember_account(app: &AppHandle, access: &str) {
    if crate::commands::drive_account_id(app).is_some() {
        return;
    }
    if let Ok((account, _, _)) = drive::who_am_i(access) {
        crate::commands::set_drive_account_id(app, Some(account));
    }
}

/// Reconcile one project with its remote, once.
pub fn once(app: &AppHandle, root: &std::path::Path) -> Result<sync::Outcome> {
    let config = sync::Config::read(root);
    if !config.is_on() {
        return Ok(sync::Outcome::default());
    }
    let backend = drive::Drive::new(token(app)?, config.folder)?;
    sync::run(root, &backend)
}

/// Start syncing the open project, making its folder on Drive if it has none.
#[tauri::command]
pub async fn start_syncing(
    app: AppHandle,
    path: std::path::PathBuf,
) -> Result<Status, crate::commands::CmdError> {
    let handle = app.clone();
    let at = path.clone();
    let outcome = crate::commands::off_thread(move || {
        if !store::is_project(&path) {
            bail!("{} is not a Lazuli project", path.display());
        }
        let access = token(&handle)?;
        // A project that already has a folder keeps it. Making a second one
        // would strand everything in the first, and — until the base learned to
        // check which remote it belongs to — would have read the empty new
        // folder as "the other side deleted everything".
        let existing = sync::Config::read(&path);
        let folder = if existing.is_on() {
            existing.folder
        } else {
            // Named after the project rather than its folder, so a person
            // looking at their Drive can tell what it is.
            let name = store::read_meta(&path)
                .map(|meta| meta.name)
                .unwrap_or_else(|_| "Lazuli project".to_owned());
            let folder = drive::Drive::make_project_folder(&access, &name)?;
            sync::Config {
                folder: folder.clone(),
            }
            .write(&path)?;
            folder
        };
        let backend = drive::Drive::new(access, folder)?;
        sync::run(&path, &backend)
    })
    .await;
    // Read back rather than assumed: a sign-in that was never made, or a folder
    // that could not be created, leaves the project syncing nowhere, and a
    // button that says otherwise is worse than the failure it is hiding.
    Ok(Status {
        on: sync::Config::read(&at).is_on(),
        problem: problem_in(outcome),
    })
}

/// What to tell the user, whether the pass failed outright or only in part.
///
/// A pass that carried nine files and dropped one is not a success, and saying
/// nothing about it would leave a photograph quietly not on the other machine.
fn problem_in(outcome: Result<sync::Outcome>) -> Option<String> {
    match outcome {
        Err(err) => Some(format!("{err:#}")),
        Ok(outcome) if outcome.failed > 0 => Some(format!(
            "{} of them did not go through — {}",
            outcome.failed,
            outcome.problem.unwrap_or_default()
        )),
        Ok(_) => None,
    }
}

#[tauri::command]
pub fn stop_syncing(path: std::path::PathBuf) -> Result<Status, crate::commands::CmdError> {
    sync::turn_off(&path)?;
    Ok(Status::default())
}

#[tauri::command]
pub fn sync_status(path: std::path::PathBuf) -> Status {
    Status {
        on: sync::Config::read(&path).is_on(),
        problem: None,
    }
}

/// Reconcile now, rather than waiting for the timer.
#[tauri::command]
pub async fn sync_now(
    app: AppHandle,
    path: std::path::PathBuf,
) -> Result<Status, crate::commands::CmdError> {
    let handle = app.clone();
    let at = path.clone();
    let outcome = crate::commands::off_thread(move || once(&handle, &path)).await;
    Ok(Status {
        on: sync::Config::read(&at).is_on(),
        problem: problem_in(outcome),
    })
}

/// A sync loop for one open project, which stops when the project closes.
pub struct Loop {
    stop: Arc<AtomicBool>,
}

impl Loop {
    /// Start reconciling `root` every [`EVERY`], in the background.
    ///
    /// Does nothing at all for a project that syncs nowhere, which is the
    /// ordinary case and must cost nothing.
    pub fn start(app: AppHandle, root: std::path::PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                // Checked every time round rather than once: the user can turn
                // it on while the project is open.
                if sync::Config::read(&root).is_on() {
                    match once(&app, &root) {
                        Err(err) => eprintln!("lazuli: sync failed: {err:#}"),
                        Ok(outcome) if outcome.failed > 0 => eprintln!(
                            "lazuli: {} file(s) did not sync: {}",
                            outcome.failed,
                            outcome.problem.unwrap_or_default()
                        ),
                        Ok(_) => {}
                    }
                }
                // In short naps, so closing the project does not wait a minute
                // for the thread to notice.
                for _ in 0..EVERY.as_secs() {
                    if flag.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        });
        Self { stop }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Loop {
    fn drop(&mut self) {
        self.stop();
    }
}

// --- sharing ----------------------------------------------------------------

/// Who a project is shared with, and what to tell them to look for.
///
/// `invite` is the Drive folder's own name, which the person invited pastes
/// into the chooser. It travels with the member list because both are read in
/// the same breath and the dialog shows them together.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Sharing {
    pub invite: String,
    pub members: Vec<drive::Member>,
}

/// Empty for a project that syncs nowhere: there is nobody to share a folder
/// that does not exist with.
#[tauri::command]
pub async fn project_members(
    app: AppHandle,
    path: std::path::PathBuf,
) -> Result<Sharing, crate::commands::CmdError> {
    let handle = app.clone();
    Ok(crate::commands::off_thread(move || {
        let config = sync::Config::read(&path);
        if !config.is_on() {
            return Ok(Sharing {
                invite: String::new(),
                members: Vec::new(),
            });
        }
        let access = token(&handle)?;
        Ok(Sharing {
            invite: drive::folder_name(&access, &config.folder)?,
            members: drive::members(&access, &config.folder)?,
        })
    })
    .await?)
}

#[tauri::command]
pub async fn share_project(
    app: AppHandle,
    path: std::path::PathBuf,
    email: String,
    role: String,
) -> Result<Sharing, crate::commands::CmdError> {
    let handle = app.clone();
    let at = path.clone();
    crate::commands::off_thread(move || {
        let config = sync::Config::read(&path);
        if !config.is_on() {
            bail!("this project has to be synced before it can be shared");
        }
        drive::share_with(&token(&handle)?, &config.folder, email.trim(), &role)
    })
    .await?;
    project_members(app, at).await
}

#[tauri::command]
pub async fn unshare_project(
    app: AppHandle,
    path: std::path::PathBuf,
    permission: String,
) -> Result<Sharing, crate::commands::CmdError> {
    let handle = app.clone();
    let at = path.clone();
    crate::commands::off_thread(move || {
        let config = sync::Config::read(&path);
        if !config.is_on() {
            bail!("this project is not shared with anyone");
        }
        drive::unshare(&token(&handle)?, &config.folder, &permission)
    })
    .await?;
    project_members(app, at).await
}

/// Set, or clear, what this user is called in one project alone.
#[tauri::command]
pub fn set_my_name_here(
    app: AppHandle,
    state: tauri::State<crate::commands::AppState>,
    name: Option<String>,
) -> Result<(), crate::commands::CmdError> {
    crate::commands::set_display_name_here(&app, &state, name.as_deref())?;
    Ok(())
}

/// Take a project somebody shared, through Google's own file chooser.
///
/// The Picker is the only way in: `drive.file` cannot see a folder the app did
/// not create, so the user hands this one over once and the grant sticks.
///
/// What comes back is a folder id, and the rest is the ordinary sync: a local
/// folder is made for it, pointed at that remote, and filled by a pass. The
/// text arrives before the photographs, so the timeline is readable at once.
#[tauri::command]
pub async fn add_shared_project(
    app: AppHandle,
    looking_for: String,
) -> Result<Option<std::path::PathBuf>, crate::commands::CmdError> {
    let handle = app.clone();
    let picked = crate::commands::off_thread(move || {
        let access = token(&handle)?;
        drive::pick_folder(&access, looking_for.trim())
    })
    .await?;
    let Some(picked) = picked else {
        return Ok(None);
    };

    let parent = crate::commands::default_projects_dir(app.clone());
    let handle = app.clone();
    let path = crate::commands::off_thread(move || {
        // Named after the Drive folder, and given a free name if that is taken:
        // a project arriving from somebody else must not land on one of ours.
        let path = crate::paths::unique_path(&parent, &crate::paths::folder_name_for(&picked.name));
        std::fs::create_dir_all(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        sync::Config {
            folder: picked.id.clone(),
        }
        .write(&path)?;

        let backend = drive::Drive::new(token(&handle)?, picked.id)?;
        sync::run(&path, &backend)?;

        // A folder that came down without a `lazuli.yaml` is not a project —
        // the wrong folder was chosen, or it was shared empty. Say so rather
        // than leaving an unopenable folder behind.
        if !store::is_project(&path) {
            let _ = std::fs::remove_dir_all(&path);
            bail!(
                "{} does not hold a Lazuli project. Choose the folder that has \
                 a lazuli.yaml in it.",
                picked.name
            );
        }
        Ok(path)
    })
    .await?;

    crate::commands::file_project_at_top(&app, path.clone());
    Ok(Some(path))
}
