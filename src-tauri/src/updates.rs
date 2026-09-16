//! Keeping Lapis up to date without the user ever seeing it happen.
//!
//! The shape: check a few seconds after launch, download in the background if
//! there is something, and apply it at a moment the user cannot notice. Which
//! moment differs by platform, and that difference is the whole point of this
//! module:
//!
//! - On **macOS and Linux** the updater plugin swaps the files in place, inside
//!   this process, and returns. So the update is applied as the window closes,
//!   and the next launch is the new version. Nothing else is running that could
//!   collide with it.
//! - On **Windows** a running executable cannot be replaced, so applying an
//!   update means handing over to the NSIS installer and exiting. Doing that at
//!   close is a race: the installer takes a few seconds, and it force-closes any
//!   Lapis it finds — including one the user has just reopened, which is what
//!   "the app closed by itself" was. Worse, a Lapis started *after* that check
//!   holds the executable while the installer tries to overwrite it, and the
//!   install aborts half done. So on Windows the download is kept on disk and
//!   applied at the *start* of the next launch, before any window exists: the
//!   user's own double-click is what triggers the install, the installer
//!   relaunches Lapis when it is done, and the only visible trace is that one
//!   launch takes a couple of seconds longer to show a window.
//!
//! Either way the version in front of the user changes only between sessions,
//! never during one, and never with a prompt.
//!
//! Nothing the endpoint says is trusted: the plugin verifies an Ed25519
//! signature over the download against the public key in `tauri.conf.json` and
//! refuses anything else. A hijacked endpoint can withhold updates; it cannot
//! deliver one. The installer kept on disk between sessions has already passed
//! that check, and sits in the user's own config folder — the same trust
//! boundary as the installed executable itself.

use std::time::Duration;

use anyhow::Result;
use tauri::AppHandle;
#[cfg(not(windows))]
use tauri::Manager;
use tauri_plugin_updater::{Update, UpdaterExt};

/// How long after launch the check runs.
///
/// Not on the first frame: startup is already competing for the network and the
/// disk, and nothing here is acted on before the next session anyway.
const CHECK_DELAY: Duration = Duration::from_secs(4);

/// The first thing `setup` does, before the window is built.
///
/// On Windows this is where a downloaded update is applied — see the module
/// docs for why it has to be here and not at close. On the other platforms it
/// only prepares the slot the download will be staged in.
pub fn at_launch(app: &AppHandle) {
    #[cfg(windows)]
    on_disk::apply_pending(app);
    #[cfg(not(windows))]
    app.manage(Staged::default());
}

/// Start the background check, and forget about it.
///
/// Every failure from here on is swallowed. The user did not ask for any of
/// this, so a machine that is offline, or behind a proxy that eats the request,
/// or pointed at an endpoint that has moved, must not be interrupted to be
/// told — it simply stays on the version it has. The cost of that choice is
/// that a permanently broken endpoint is invisible, which is why cutting a
/// release is followed by watching one real install update.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(CHECK_DELAY);
        let _ = tauri::async_runtime::block_on(check_and_stage(&app));
    });
}

async fn check_and_stage(app: &AppHandle) -> Result<()> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(());
    };
    stage(app, update).await
}

/// Installs a staged update as the window goes away.
///
/// A no-op on Windows, where the update was applied at launch instead. On the
/// other platforms the plugin replaces the files in place and returns, so the
/// close is not held up and the process goes away as it was going to — running
/// the old binary until the end, which is fine, and the new one next time.
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    #[cfg(not(windows))]
    if let tauri::WindowEvent::CloseRequested { .. } = event {
        let staged = window
            .app_handle()
            .state::<Staged>()
            .0
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some((update, bytes)) = staged {
            // Failing to install is not a reason to trap someone in the app;
            // the next session will download it again.
            let _ = update.install(&bytes);
        }
    }
    #[cfg(windows)]
    let _ = (window, event);
}

// --- macOS and Linux: hold the bytes until the window closes -----------------

/// A verified download, waiting for the window to close.
#[cfg(not(windows))]
#[derive(Default)]
struct Staged(std::sync::Mutex<Option<(Update, Vec<u8>)>>);

#[cfg(not(windows))]
async fn stage(app: &AppHandle, update: Update) -> Result<()> {
    let bytes = update.download(|_, _| {}, || {}).await?;
    if let Ok(mut slot) = app.state::<Staged>().0.lock() {
        *slot = Some((update, bytes));
    }
    Ok(())
}

// --- Windows: keep the installer on disk until the next launch ---------------

#[cfg(windows)]
async fn stage(app: &AppHandle, update: Update) -> Result<()> {
    on_disk::stage(app, update).await
}

#[cfg(windows)]
mod on_disk {
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use anyhow::{Context, Result};
    use semver::Version;
    use serde::{Deserialize, Serialize};
    use tauri::{AppHandle, Manager};
    use tauri_plugin_updater::Update;

    /// The downloaded NSIS installer, named without its version so there is
    /// only ever one and a newer download replaces an older one.
    const INSTALLER: &str = "Lapis-setup.exe";
    /// Written last, once the installer is completely on disk. Its presence is
    /// what says "there is something to apply".
    const PENDING: &str = "pending.json";
    /// `PENDING`, renamed the moment a launch decides to apply it.
    const INSTALLING: &str = "installing.json";

    #[derive(Serialize, Deserialize)]
    struct Pending {
        version: String,
    }

    /// `%APPDATA%\<identifier>\update\`, beside `settings.json`.
    fn dir(app: &AppHandle) -> Result<PathBuf> {
        Ok(app
            .path()
            .app_config_dir()
            .context("no config dir")?
            .join("update"))
    }

    fn read_pending(dir: &Path) -> Option<Version> {
        let bytes = fs::read(dir.join(PENDING)).ok()?;
        let pending: Pending = serde_json::from_slice(&bytes).ok()?;
        Version::parse(&pending.version).ok()
    }

    /// Download the installer to disk, unless a download of that version or a
    /// newer one is already waiting.
    pub(super) async fn stage(app: &AppHandle, update: Update) -> Result<()> {
        let dir = dir(app)?;
        let offered = Version::parse(&update.version)?;
        if read_pending(&dir).is_some_and(|have| have >= offered) {
            return Ok(());
        }

        let bytes = update.download(|_, _| {}, || {}).await?;

        fs::create_dir_all(&dir)?;
        // Written under another name and renamed into place, so a crash or a
        // full disk mid-write cannot leave a truncated installer under the
        // name the next launch trusts.
        let part = dir.join("Lapis-setup.part");
        fs::write(&part, &bytes)?;
        fs::rename(&part, dir.join(INSTALLER))?;
        // Only now; until this exists, nothing is pending.
        let pending = Pending {
            version: update.version.clone(),
        };
        fs::write(dir.join(PENDING), serde_json::to_vec(&pending)?)?;
        Ok(())
    }

    /// What a launch should do about the update folder.
    #[derive(Debug, PartialEq, Eq)]
    enum Action {
        /// Nothing there; start normally.
        Start,
        /// Whatever is there is spent or unusable; remove it and start normally.
        CleanUp,
        /// A newer version is waiting: hand over to its installer.
        Install,
    }

    /// The decision, kept apart from its effects so it can be tested — it is
    /// the one place a mistake becomes an app that never shows a window.
    fn decide(
        installing: bool,
        pending: Option<&Version>,
        installer_present: bool,
        current: &Version,
    ) -> Action {
        // `installing.json` means a previous launch handed over to the
        // installer. Either this is what came back — the new version, and the
        // folder is spent — or the installer never finished. Retrying the
        // second case would try again on every launch, forever, and never
        // show a window; so both cases clean up and start, and a fresh
        // download happens in the background if one is still due.
        if installing {
            return Action::CleanUp;
        }
        match pending {
            None => Action::Start,
            Some(version) if version > current && installer_present => Action::Install,
            Some(_) => Action::CleanUp,
        }
    }

    /// Apply a waiting update, if there is one, and do not return if there is.
    pub(super) fn apply_pending(app: &AppHandle) {
        let Ok(dir) = dir(app) else { return };
        let pending = read_pending(&dir);
        let action = decide(
            dir.join(INSTALLING).exists(),
            pending.as_ref(),
            dir.join(INSTALLER).exists(),
            &app.package_info().version,
        );
        match action {
            Action::Start => {}
            // Best effort: the installer that relaunched us may still be
            // exiting and holding its own file. The next launch gets it.
            Action::CleanUp => {
                let _ = fs::remove_dir_all(&dir);
            }
            Action::Install => install(&dir),
        }
    }

    /// Hand over to the installer and exit. Returns only if it could not start.
    fn install(dir: &Path) {
        // Claim it first, so a second double-click in the seconds the
        // installer takes finds nothing pending, starts normally, is closed
        // by the installer like any other running instance, and is replaced
        // by the one the installer relaunches. Two installers would fight.
        if fs::rename(dir.join(PENDING), dir.join(INSTALLING)).is_err() {
            return;
        }

        // The same switches the updater plugin passes: silent, this is an
        // update rather than a first install, and relaunch when done. The
        // flags are read by Tauri's NSIS template, not by NSIS itself.
        let mut command = Command::new(dir.join(INSTALLER));
        command.args(["/S", "/UPDATE", "/R"]);

        // `lapis <folder>` should come back as `lapis <folder>`. The template
        // hands whatever follows `/ARGS` to the relaunched executable, so the
        // arguments are escaped the way the plugin does and passed raw —
        // `Command` would otherwise quote them a second time.
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        if !args.is_empty() {
            use std::os::windows::process::CommandExt as _;
            command.raw_arg("/ARGS");
            for arg in &args {
                command.raw_arg(escape_for_nsis(arg));
            }
        }

        if command.spawn().is_ok() {
            std::process::exit(0);
        }
        // Could not start it. `installing.json` is left in place on purpose:
        // the next launch reads it as a spent attempt and cleans up, rather
        // than trying the same broken installer again.
    }

    /// Quote an argument for the NSIS command line, as the updater plugin does.
    ///
    /// Like ordinary Windows quoting — double quotes around anything with
    /// whitespace, backslashes doubled only before a quote — plus one more
    /// trigger: a `/` anywhere, because NSIS would otherwise read the argument
    /// as the start of a switch of its own.
    fn escape_for_nsis(arg: &OsStr) -> OsString {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        let needs_quotes = arg.is_empty()
            || arg
                .as_encoded_bytes()
                .iter()
                .any(|byte| matches!(byte, b' ' | b'\t' | b'/'));

        let mut out: Vec<u16> = Vec::new();
        if needs_quotes {
            out.push(u16::from(b'"'));
        }
        let mut backslashes = 0;
        for unit in arg.encode_wide() {
            if unit == u16::from(b'\\') {
                backslashes += 1;
                out.push(unit);
                continue;
            }
            if unit == u16::from(b'"') {
                // The backslashes just written now precede a quote, so each
                // needs a partner, and the quote needs one of its own.
                out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes + 1));
            }
            backslashes = 0;
            out.push(unit);
        }
        if needs_quotes {
            // Same for trailing backslashes, which would otherwise escape the
            // closing quote.
            out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
            out.push(u16::from(b'"'));
        }
        OsString::from_wide(&out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn v(s: &str) -> Version {
            Version::parse(s).unwrap()
        }

        #[test]
        fn nothing_pending_starts_normally() {
            assert_eq!(decide(false, None, false, &v("0.1.1")), Action::Start);
        }

        #[test]
        fn a_newer_installer_is_applied() {
            assert_eq!(
                decide(false, Some(&v("0.1.2")), true, &v("0.1.1")),
                Action::Install
            );
        }

        #[test]
        fn the_version_that_came_back_is_spent() {
            // The installer relaunched us and we are now 0.1.2: the folder
            // that got us here is done with.
            assert_eq!(
                decide(false, Some(&v("0.1.2")), true, &v("0.1.2")),
                Action::CleanUp
            );
        }

        #[test]
        fn a_pending_json_without_its_installer_is_cleaned_up() {
            assert_eq!(
                decide(false, Some(&v("0.1.2")), false, &v("0.1.1")),
                Action::CleanUp
            );
        }

        #[test]
        fn an_attempt_that_did_not_finish_is_never_retried() {
            // Even though everything else says "install": doing so would loop
            // on every launch and the window would never appear.
            assert_eq!(
                decide(true, Some(&v("0.1.2")), true, &v("0.1.1")),
                Action::CleanUp
            );
        }

        fn esc(s: &str) -> String {
            escape_for_nsis(OsStr::new(s)).to_string_lossy().into_owned()
        }

        #[test]
        fn plain_arguments_pass_through() {
            assert_eq!(esc(r"C:\Projects\Lapis"), r"C:\Projects\Lapis");
        }

        #[test]
        fn spaces_and_slashes_get_quotes() {
            assert_eq!(esc(r"C:\My Projects"), r#""C:\My Projects""#);
            assert_eq!(esc("a/b"), r#""a/b""#);
            assert_eq!(esc(""), r#""""#);
        }

        #[test]
        fn quotes_and_the_backslashes_before_them_are_escaped() {
            assert_eq!(esc(r#"say "hi""#), r#""say \"hi\"""#);
            assert_eq!(esc(r#"a\"b"#), r#"a\\\"b"#);
        }

        #[test]
        fn a_trailing_backslash_does_not_eat_the_closing_quote() {
            assert_eq!(esc(r"C:\My Projects\"), r#""C:\My Projects\\""#);
        }
    }
}
