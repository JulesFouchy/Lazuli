// Keeping Lapis up to date without ever saying so.
//
// The shape is: check a few seconds after launch, download in the background if
// there is something, and install it as the window closes. The user is never
// asked and never interrupted, and the version they get is the one they find
// the next time they open the app.
//
// Nothing the endpoint says is trusted. The plugin verifies an Ed25519
// signature over the downloaded installer against the public key baked into
// this build and refuses anything that does not match, so a hijacked endpoint
// can stop updates arriving and cannot make one arrive.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { check, type Update } from "@tauri-apps/plugin-updater";

/**
 * How long after launch the check runs.
 *
 * Not on the first frame: startup is already competing for the network and the
 * disk, and nothing here is urgent — the result is not acted on until the app
 * closes anyway.
 */
const CHECK_DELAY_MS = 4000;

/**
 * A downloaded update, waiting for the window to close.
 *
 * Held rather than installed immediately because installing is the one part of
 * this the user would notice: on Windows it replaces the running executable,
 * which means killing the app out from under them.
 */
let staged: Update | null = null;

/**
 * Check, download, and arrange for the install to happen on close.
 *
 * Every failure here is swallowed. The user did not ask for any of this, so a
 * machine that is offline, behind a proxy that eats the request, or pointed at
 * an endpoint that has moved must not be interrupted to be told — it simply
 * stays on the version it has. The cost of that choice is that a permanently
 * broken endpoint is invisible, which is why the release checklist says to
 * install a build and watch it update at least once.
 */
export function startUpdates(): void {
  setTimeout(() => void checkAndStage(), CHECK_DELAY_MS);
  void installOnClose();
}

async function checkAndStage(): Promise<void> {
  try {
    const update = await check();
    if (!update) return;
    await update.download();
    staged = update;
  } catch {
    // Deliberately silent; see above.
  }
}

/**
 * Install whatever is staged as the window goes away.
 *
 * `restartAfterInstall: false` is the whole point: the app has just been closed
 * on purpose, and reopening it because an update happened would be the single
 * most annoying thing this code could do.
 *
 * On Windows `install` hands over to the NSIS installer and exits this process,
 * so nothing after it runs. On macOS and Linux it swaps the files in place and
 * returns, and the close then continues normally — either way the new version
 * is what opens next time.
 */
async function installOnClose(): Promise<void> {
  // Not named `window`: that would shadow the global one for the rest of this
  // function, in a file where both are plausible.
  const appWindow = getCurrentWindow();
  await appWindow.onCloseRequested(async (event) => {
    if (!staged) return;
    // Hold the window open for the moment the handover takes. Without this the
    // process can be gone before the installer has been launched.
    event.preventDefault();
    const update = staged;
    // Cleared first, so a failure cannot leave a close that never completes.
    staged = null;
    try {
      await update.install({ restartAfterInstall: false });
    } catch {
      // An update that will not install is not a reason to trap someone in the
      // app. It stays on disk and the next launch will find it again.
    }
    await appWindow.destroy();
  });
}
