// Finding, offering and installing a new version.
//
// The check talks to a public endpoint named in `tauri.conf.json`, but nothing
// it says is trusted: the plugin verifies an Ed25519 signature over the
// downloaded installer against the public key baked into this build, and
// refuses anything that does not match. A hostile endpoint can therefore stop
// updates from arriving, and cannot make one arrive.

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

import { closeModal, openModal } from "./modal";
import { el, toast, toastError } from "./ui";

/**
 * How long after launch the silent check runs.
 *
 * Not on the first frame: startup is already competing for the network and the
 * disk, and an update the user learns about four seconds late costs nothing.
 */
const STARTUP_DELAY_MS = 4000;

/** The version this build reports, cached because it cannot change. */
let version: string | null = null;

export async function appVersion(): Promise<string> {
  version ??= await getVersion();
  return version;
}

/**
 * Look for a new version.
 *
 * `quiet` is the startup call: a machine that is offline, or behind a proxy
 * that eats the request, must not be told about it — the user did not ask.
 * The same failure when they pressed a button is worth reporting, because
 * otherwise the button looks broken.
 */
export async function checkForUpdate(quiet: boolean): Promise<void> {
  let update: Update | null;
  try {
    update = await check();
  } catch (err) {
    if (!quiet) toastError("Could not check for updates", err);
    return;
  }

  if (!update) {
    if (!quiet) toast(`Lapis ${await appVersion()} is up to date.`);
    return;
  }
  offer(update);
}

/** Run the startup check once, without ever getting in the way. */
export function checkForUpdateOnStartup(): void {
  setTimeout(() => void checkForUpdate(true), STARTUP_DELAY_MS);
}

function offer(update: Update): void {
  const progress = el("p", { class: "hint", text: "" });
  const install = el("button", {
    class: "button button--primary",
    text: "Install and restart",
  });
  const later = el("button", { class: "button", text: "Later" });

  install.onclick = () => {
    install.disabled = true;
    later.disabled = true;
    void run(update, progress);
  };
  later.onclick = () => closeModal();

  const body = el("div", { class: "modal__body-inner" });
  body.append(
    el("p", {
      text: `Lapis ${update.version} is available. You have ${update.currentVersion}.`,
    }),
  );
  // Release notes are whatever the endpoint chose to send, so they go in as
  // text and never as markup.
  if (update.body?.trim()) {
    body.append(el("pre", { class: "release-notes", text: update.body.trim() }));
  }
  body.append(progress);

  openModal({
    title: "Update available",
    body,
    foot: el(
      "div",
      { class: "modal__foot" },
      later,
      el("span", { class: "card__grow" }),
      install,
    ),
  });
}

async function run(update: Update, progress: HTMLElement): Promise<void> {
  let total = 0;
  let soFar = 0;
  try {
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          total = event.data.contentLength ?? 0;
          progress.textContent = "Downloading…";
          break;
        case "Progress":
          soFar += event.data.chunkLength;
          progress.textContent = total
            ? `Downloading… ${Math.round((soFar / total) * 100)}%`
            : `Downloading… ${Math.round(soFar / 1024 / 1024)} MB`;
          break;
        case "Finished":
          progress.textContent = "Installing…";
          break;
      }
    });
  } catch (err) {
    progress.textContent = "";
    toastError("The update could not be installed", err);
    return;
  }

  // On Windows the NSIS installer has already taken over and will restart the
  // app itself; this is what handles the platforms where it does not. It may
  // simply never return, which is fine — there is nothing after it.
  await relaunch();
}

/** Version, and the only place that offers a check the user asked for. */
export function openAboutDialog(): void {
  const line = el("p", { class: "about__version", text: "Lapis" });
  void appVersion().then((v) => {
    line.textContent = `Lapis ${v}`;
  });

  openModal({
    title: "About Lapis",
    body: el(
      "div",
      { class: "modal__body-inner" },
      el(
        "div",
        { class: "field" },
        line,
        el("p", {
          class: "hint",
          text: "A project is a folder on disk. Nothing is kept anywhere else, and nothing about you is sent anywhere — the only request Lapis makes is the one that asks whether a newer version exists.",
        }),
      ),
    ),
    foot: el(
      "div",
      { class: "modal__foot" },
      el("span", { class: "card__grow" }),
      el("button", {
        class: "button",
        text: "Check for updates",
        onclick: () => void checkForUpdate(false),
      }),
    ),
  });
}
