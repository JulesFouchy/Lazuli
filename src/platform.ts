// What this build can do, rather than what it is running on.
//
// Everything here is a capability and none of it is a platform name, because
// the three questions that look alike are not the same question:
//
// - *Does this exist here?* A capability. There is no window to minimise on a
//   phone, and no OS drag-and-drop to listen for. Asked in TypeScript.
// - *How big is the box?* A width media query. A narrow desktop window should
//   get the narrow layout too.
// - *Is this reachable?* `(hover: none)` / `(pointer: coarse)`. A Windows
//   laptop with a touchscreen needs long-press as well as right-click, and its
//   width says nothing about whether it has fingers on it.
//
// Nothing gates *behaviour* on width, and nothing gates a layout on the
// platform. Code that asks "is this Android" gets a touchscreen laptop wrong
// in one direction and a narrow window wrong in the other.
//
// Set before the first paint by the boot script in `index.html`, so the
// stylesheet can key off `[data-platform]` without a frame of the wrong one.

const platform = document.documentElement.dataset.platform ?? "desktop";

const MOBILE = platform === "android" || platform === "ios";

/**
 * A window frame the page draws itself.
 *
 * `src/titlebar.ts` and everything it needs: minimise, maximise, close, F11,
 * and dragging the window by a strip of the page. A phone's window is the
 * screen and none of it applies.
 */
export const HAS_WINDOW_CHROME = !MOBILE;

/**
 * Files arriving as paths the app can read directly.
 *
 * Explorer's drag-and-drop, and its "Copy as path" pasted as text. Both hand
 * over an absolute path to a file elsewhere on the disk, which is not a thing
 * a phone has: a picture is chosen through the system picker instead, and what
 * comes back is a grant to one file rather than a path to any.
 */
export const HAS_OS_FILE_DROP = !MOBILE;

/**
 * Back and forward as the pointing device's own gesture.
 *
 * The mouse's thumb buttons and a trackpad's sideways two-finger swipe. A
 * phone has the system back gesture instead, which is the OS's to handle and
 * not ours to imitate.
 */
export const HAS_MOUSE_HISTORY = !MOBILE;

/**
 * A back gesture the system provides and the page has to answer.
 *
 * Android's back button and its edge swipe both drive the webview's own
 * history, so the way to be asked is to have somewhere to go back *to* — see
 * `src/back.ts`. A desktop has no such gesture, and arming one there would
 * answer Alt+Left twice: once through the key and once through the history it
 * moves.
 */
export const HAS_SYSTEM_BACK = MOBILE;

/**
 * Whether the thing pointing at the page right now is a finger.
 *
 * Asked each time rather than answered once: a laptop can have both a
 * touchscreen and a mouse, and which one is in use can change between one
 * interaction and the next.
 */
export const coarsePointer = (): boolean =>
  window.matchMedia("(pointer: coarse)").matches;
