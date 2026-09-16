// The window's title bar, drawn by the page rather than by the OS.
//
// It is hidden until the pointer reaches the top edge of the window, and then
// slides down *over* the page instead of pushing it: at rest the bar costs no
// height at all, and the mechanical move of going straight to the top-right
// corner and clicking still lands on Close.
//
// Showing and hiding is CSS `:hover` on the strip along the top, not a
// `mousemove` listener — the pointer arriving over the bar itself is then the
// same condition that keeps it down, with no hysteresis to get wrong.

import { getCurrentWindow } from "@tauri-apps/api/window";

import { el, toastError } from "./ui";

const appWindow = getCurrentWindow();

/** Build the bar and put it on the page. Called once, at startup. */
export function startTitlebar(): void {
  const maximise = control("Maximise", "maximise", () =>
    appWindow.toggleMaximize(),
  );

  const bar = el(
    "div",
    // Tauri's own handler reads this attribute off the element the press
    // landed on, so it goes on every part of the bar that is not a button:
    // dragging moves the window, and a double-click maximises it.
    { class: "titlebar", "data-tauri-drag-region": "" },
    // No name and no icon: the window is the app, and both are already on the
    // taskbar button. What is left is the part of the bar you drag it by.
    el("div", { class: "titlebar__drag", "data-tauri-drag-region": "" }),
    el(
      "div",
      { class: "titlebar__controls" },
      control("Minimise", "minimise", () => appWindow.minimize()),
      maximise,
      control("Close", "close", () => appWindow.close(), true),
    ),
  );

  document.body.append(el("div", { class: "titlebar-zone" }, bar));

  // The middle button is the one control that is about a state rather than an
  // action, so it has to be told when the state changes — including when it
  // changes without it, by a drag to the top of the screen or by Win+Up.
  const followMaximised = () =>
    void appWindow.isMaximized().then((maximised) => {
      maximise.classList.toggle("titlebar__control--restore", maximised);
      maximise.title = maximised ? "Restore" : "Maximise";
    });
  followMaximised();
  void appWindow.onResized(followMaximised);
}

function control(
  label: string,
  glyph: string,
  run: () => Promise<void>,
  danger = false,
): HTMLButtonElement {
  return el(
    "button",
    {
      class: `titlebar__control titlebar__control--${glyph}${danger ? " titlebar__control--danger" : ""}`,
      title: label,
      "aria-label": label,
      onclick: () =>
        void run().catch((err) =>
          toastError(`Could not ${label.toLowerCase()} the window`, err),
        ),
    },
    el("span", { class: "titlebar__glyph" }),
  );
}
