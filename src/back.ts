// Android's back button and its edge swipe, answered by the page.
//
// Both of them arrive the same way, and it is not an event anyone sends us.
// wry's `WryActivity` registers an `OnBackPressedCallback` that does this:
//
//     if (mWebView.canGoBack()) mWebView.goBack()
//     else { isEnabled = false; onBackPressed() }
//
// So a back press walks the *webview's own history* when there is any, and
// finishes the activity when there is not. There is nothing to listen for and
// no plugin to add: the way to be asked is to have somewhere to go back to, and
// the way to hear about it is `popstate`.
//
// Hence one sentinel entry, pushed and re-pushed. Each press spends it, the
// page goes up a layer, and a new one is pushed only while there is another
// layer left. At the launch screen with nothing open there is none, so the next
// press falls through to the activity and closes the app — which is what
// Android users expect at the root of a task, and trapping back forever is how
// an app earns one star.
//
// This lives in its own file and not in `main.ts` for a concrete reason:
// `main.ts` declares `const history: Place[]`, which shadows `window.history`
// for that entire module. A `history.pushState` written there is a `TypeError`
// at runtime, not a compile error.

import { HAS_SYSTEM_BACK } from "./platform";

/** Marks the entry as ours, so a stray one from elsewhere is not mistaken for it. */
const SENTINEL = { lazuli: "back" };

/**
 * Start answering the system's back gesture.
 *
 * `goUp` is the same one layer out that Escape and the mouse's back button do.
 * `canGoUp` is asked *after* it, to decide whether to stay armed: every layer
 * it reports on — the menu, a dialog, a field being edited, the place stack —
 * changes synchronously, so the answer is already true by then.
 */
export function startBack(options: {
  goUp: () => void;
  canGoUp: () => boolean;
}): void {
  // A desktop has no such gesture, and no window chrome to offer one.
  if (!HAS_SYSTEM_BACK) return;

  const arm = () => window.history.pushState(SENTINEL, "");

  window.addEventListener("popstate", () => {
    options.goUp();
    if (options.canGoUp()) arm();
  });

  arm();
}
