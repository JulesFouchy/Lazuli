// App shell: the launch screen, the project view, and the global keys.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import type { Entry, Project, RecentProject } from "./api";
import {
  assetUrl,
  createEntry,
  defaultProjectsDir,
  forgetRecent,
  openProject,
  recentProjects,
  restoreProject,
  restoreRecent,
  setProjectName,
  startupProject,
  trashProject,
  undoDelete,
} from "./api";
import { openAppearanceDialog } from "./appearance";
import { whileBusy } from "./busy";
import {
  closeContextMenu,
  isContextMenuOpen,
  openContextMenu,
} from "./context-menu";
import { formatRealWorld, onDateFormatChange } from "./dates";
import {
  announceDeletion,
  deleteEntry,
  openCoverPicker,
  openEntryEditor,
  refreshCoverPicker,
  refreshEntryEditor,
  type EditorContext,
} from "./entry-editor";
import { openExportDialog } from "./export-dialog";
import { addDroppedPaths } from "./image-picker";
import {
  openLightbox,
  refreshLightbox,
  type ViewerContext,
} from "./lightbox";
import { closeModal, isModalOpen, onModalDismissed } from "./modal";
import { isDeleting, markDeleting, projectKey, unmarkDeleting } from "./pending";
import { openNewProjectDialog, openStartDateEditor } from "./project-setup";
import { startTheme } from "./theme";
import { displayedEntries, renderTimeline } from "./timeline";
import { clear, el, isEditing, toast, toastError } from "./ui";

function appRoot(): HTMLElement {
  const node = document.getElementById("app");
  if (!node) throw new Error("#app is missing from index.html");
  return node;
}

const root = appRoot();

/** Everything the view needs. Rebuilt wholesale on every change. */
interface AppState {
  project: Project | null;
  newestFirst: boolean;
}

const state: AppState = {
  project: null,
  newestFirst: localStorage.getItem("journaley.newestFirst") !== "false",
};

/** The recents list as last read, so redrawing the launch screen is instant. */
let knownRecents: RecentProject[] | null = null;

/** How many reads have been asked for, so a stale answer can be spotted. */
let recentsAsked = 0;

/**
 * Read the recents list into the cache. The newest read asked for wins.
 *
 * Several of these are in flight at once — a paint, a delete finishing, an undo
 * finishing — and they do not come back in the order they were asked for. An
 * older answer landing last puts a row back in the cache that the newer one
 * knows is gone, and the next paint shows it.
 */
async function readRecents(): Promise<void> {
  const asked = ++recentsAsked;
  const recents = await whileBusy(recentProjects());
  if (asked === recentsAsked) knownRecents = recents;
}

// --- rendering -----------------------------------------------------------

function render(): void {
  clear(root);
  root.append(state.project ? projectView(state.project) : launchView());
}

function launchView(): HTMLElement {
  const view = el(
    "div",
    { class: "launch" },
    el("h1", { class: "launch__title", text: "Journaley" }),
    el("p", {
      class: "launch__tagline",
      text: "A picture and a sentence for every day you worked on it.",
    }),
    el(
      "div",
      { class: "launch__actions" },
      el("button", {
        class: "button button--primary",
        text: "New project…",
        onclick: () => newProject(),
      }),
      el("button", {
        class: "button",
        text: "Open folder…",
        onclick: () => void openFolder(),
      }),
      el("span", { class: "launch__spacer" }),
      el("button", {
        class: "button button--ghost",
        text: "Appearance…",
        onclick: () => openHere({ kind: "appearance" }),
      }),
    ),
    el("div", { class: "launch__heading", text: "Recent" }),
  );

  const list = el("div", { class: "recent" });
  view.append(list);

  const paint = (recents: RecentProject[]) => {
    const rows = recents.filter(
      (recent) => !isDeleting(projectKey(recent.path)),
    );
    for (const { at, recent } of reappearing.values()) {
      // The list it is waiting for can arrive before the undo that asked for
      // it has finished, and one row is wanted, not two.
      if (rows.some((row) => row.path === recent.path)) continue;
      rows.splice(Math.min(at, rows.length), 0, recent);
    }
    list.className = rows.length === 0 ? "empty" : "recent";
    list.replaceChildren(
      ...(rows.length === 0
        ? [el("p", { text: "Nothing opened yet." })]
        : rows.map(recentRow)),
    );
  };

  // Painted from the last list before the fresh one is asked for. Reading the
  // recents is a round trip, and without this every redraw of this screen —
  // including the one that hides a project being deleted — showed an empty
  // list for a frame first.
  if (knownRecents) paint(knownRecents);
  void readRecents()
    .then(() => paint(knownRecents ?? []))
    // Without this a failure leaves the screen looking like a first run. It
    // happens: the window can be up and asking before the backend is ready.
    .catch((err) => toastError("Could not read the recent projects", err));

  return view;
}

/**
 * One project on the launch screen, behind its own cover.
 *
 * The whole row is the button. Delete and Forget are on the right-click menu
 * rather than beside the name: both are rare, and a destructive control sitting
 * permanently next to the thing you actually came to click is one you
 * eventually hit by accident.
 */
function recentRow(recent: RecentProject): HTMLElement {
  return el(
    "button",
    {
      // The white, outlined label only makes sense over a photograph, so a row
      // without a cover is left as a plain surface in whichever theme.
      class: `recent__row${recent.cover ? " recent__row--cover" : ""}`,
      onclick: () => void openRecent(recent.path),
      oncontextmenu: (event: Event) =>
        openContextMenu(event as MouseEvent, [
          {
            label: "Open in Explorer",
            // The folder itself, not the folder selected in its parent: what
            // you want from here is to be inside it.
            run: () =>
              void openPath(recent.path).catch((err) =>
                toastError("Could not open that folder", err),
              ),
          },
          {
            label: "Delete",
            danger: true,
            run: () => deleteProject(recent),
          },
          { label: "Forget", run: () => forgetProject(recent) },
        ]),
    },
    recent.cover
      ? el("img", {
          class: "recent__image",
          src: assetUrl(recent.path, "cover", recent.cover),
          alt: "",
          decoding: "async",
        })
      : null,
    el(
      "div",
      { class: "recent__label" },
      el("div", { class: "recent__name", text: recent.name }),
      el("div", { class: "recent__path", text: recent.path }),
    ),
  );
}

function projectView(project: Project): HTMLElement {
  return el("div", {}, banner(project), timelineSection(project));
}

function banner(project: Project): HTMLElement {
  const cover = project.meta.cover;

  const nameField = el("h1", {
    class: "banner__name",
    contenteditable: "true",
    spellcheck: "false",
    text: project.meta.name,
  });
  nameField.addEventListener("click", (event) => event.stopPropagation());
  nameField.addEventListener("blur", () => {
    const name = nameField.textContent?.trim() ?? "";
    if (name && name !== project.meta.name) {
      void setProjectName(name).catch((err) =>
        toastError("Could not rename the project", err),
      );
    } else {
      nameField.textContent = project.meta.name;
    }
  });
  nameField.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      nameField.blur();
    }
  });

  return el(
    "header",
    {
      class: `banner${cover ? " banner--cover" : ""}`,
      title: "Click to change the cover",
      onclick: () => openHere({ kind: "cover" }),
    },
    cover
      ? el("img", {
          class: "banner__image",
          src: assetUrl(project.root, "cover", cover),
          alt: `${project.meta.name} cover`,
          decoding: "async",
        })
      : null,
    el("div", { class: "banner__hint", text: "Click to change the cover" }),
    // No buttons over the cover: Escape and the mouse's Back button leave the
    // project, and Reveal in Explorer is on the project's own right-click menu
    // on the launch screen.
    el(
      "div",
      { class: "banner__bar" },
      nameField,
      el(
        "div",
        { class: "banner__meta" },
        el("span", {
          text: `${project.entries.length} ${
            project.entries.length === 1 ? "entry" : "entries"
          } · `,
        }),
        el("button", {
          class: "banner__start",
          // "started" is part of the target: it is the label for the date, and
          // aiming at eleven characters of date is a small thing to ask.
          text: `started ${formatRealWorld(project.meta.start_date)}`,
          title: "Change the start date",
          onclick: (event: Event) => {
            // The banner itself opens the cover picker.
            event.stopPropagation();
            openHere({ kind: "start-date" });
          },
        }),
      ),
    ),
  );
}

function timelineSection(project: Project): HTMLElement {
  return el(
    "main",
    { class: "timeline" },
    el(
      "div",
      { class: "timeline__toolbar" },
      el("button", {
        class: "button button--primary",
        text: "New entry",
        onclick: () => void addEntry(),
      }),
      el("button", {
        class: "button",
        text: "Export video…",
        onclick: () => openHere({ kind: "export" }),
      }),
      el("span", { class: "timeline__spacer" }),
      el("button", {
        class: "button button--ghost",
        text: "Appearance…",
        onclick: () => openHere({ kind: "appearance" }),
      }),
      el("button", {
        class: "button button--ghost",
        text: state.newestFirst ? "Newest first ↓" : "Oldest first ↑",
        title: "Reverse the timeline",
        onclick: () => {
          state.newestFirst = !state.newestFirst;
          localStorage.setItem(
            "journaley.newestFirst",
            String(state.newestFirst),
          );
          render();
        },
      }),
    ),
    renderTimeline(
      project,
      {
        viewEntry: (entry: Entry) => openHere({ kind: "view", id: entry.id }),
        editEntry: (entry: Entry) => openHere({ kind: "entry", id: entry.id }),
        deleteEntry: (entry: Entry) =>
          deleteEntry(entry.id, editorContext),
      },
      state.newestFirst,
    ),
  );
}

// --- the context the editors close over -----------------------------------

const editorContext: EditorContext = {
  project: () => {
    if (!state.project) throw new Error("no project is open");
    return state.project;
  },
  entry: (id) => state.project?.entries.find((e) => e.id === id) ?? null,
  refresh: () => render(),
  noteDeletion: (what, deleted) => {
    const offer: UndoOffer = { dismiss: () => {} };
    offer.dismiss = announceDeletion(what, deleted, () => void runUndo(), () => {
      const at = liveOffers.indexOf(offer);
      if (at >= 0) liveOffers.splice(at, 1);
    });
    liveOffers.push(offer);
  },
};

const viewerContext: ViewerContext = {
  project: () => {
    if (!state.project) throw new Error("no project is open");
    return state.project;
  },
  entry: (id) => state.project?.entries.find((e) => e.id === id) ?? null,
  entries: () =>
    state.project ? displayedEntries(state.project, state.newestFirst) : [],
  moved: (id) => noteViewerMoved(id),
};

interface UndoOffer {
  dismiss: () => void;
}

/**
 * Deletion toasts still on screen, oldest first.
 *
 * The Rust undo stack pops the last thing trashed, which is the one the newest
 * of these describes, so taking that one down on Ctrl+Z keeps the two in step:
 * every toast left is still offering exactly the undo it names.
 */
const liveOffers: UndoOffer[] = [];

// --- actions ---------------------------------------------------------------

async function openFolder(): Promise<void> {
  const chosen = await openDialog({
    directory: true,
    title: "Open a Journaley project",
    defaultPath: await defaultProjectsDir(),
  });
  if (typeof chosen === "string") await openRecent(chosen);
}

function newProject(): void {
  openHere({ kind: "new-project" });
}

/** How many opens have been asked for, so a stale answer can be spotted. */
let opensAsked = 0;

/**
 * Open a project folder, reporting whether it worked.
 *
 * Opens run off the main thread in Rust and so are no longer serialised: two
 * quick clicks are two opens in flight, and they need not come back in order.
 * Only the newest one asked for gets to become the project on screen — it is
 * also the one Rust ends up holding open, so the two stay in step.
 */
async function loadProject(path: string): Promise<boolean> {
  const asked = ++opensAsked;
  try {
    const project = await whileBusy(openProject(path));
    if (asked !== opensAsked) return false;
    state.project = project;
    render();
    return true;
  } catch (err) {
    if (asked === opensAsked) toastError("Could not open that folder", err);
    return false;
  }
}

function openRecent(path: string): Promise<void> {
  return goTo({ project: path, modal: null });
}

// --- deleting and forgetting a project -------------------------------------
//
// Both are undone from the toast rather than from the Rust undo stack, which
// belongs to an open project and so is not reachable from the launch screen.
// `launchUndo` is what makes Ctrl+Z work here too.

/** The last thing done on the launch screen that can still be taken back. */
let launchUndo: (() => void) | null = null;

function offerUndo(message: string, undo: () => void): void {
  const dismiss = toast(message, {
    action: {
      label: "Undo",
      run: () => {
        launchUndo = null;
        undo();
      },
    },
  });
  // Ctrl+Z does the same thing the button does, so it takes the offer down
  // with it: a toast still offering an undo that has already happened would
  // undo whatever came before it instead.
  launchUndo = () => {
    dismiss();
    undo();
  };
}

/**
 * Projects an undo has asked back, shown until the list itself has them again.
 *
 * The mirror of `pending.ts`: that one hides rows the backend still has, this
 * one shows rows it does not have yet. Undo can be pressed before the delete it
 * undoes has even reached the Recycle Bin, and the row should be back the
 * moment it is pressed rather than after two round trips.
 */
const reappearing = new Map<string, { at: number; recent: RecentProject }>();

/** Put a row back on screen at once, and do the real restore behind it. */
function undoRemoval(
  recent: RecentProject,
  at: number,
  key: string,
  removal: Promise<number | null>,
  restore: (index: number) => Promise<unknown>,
): void {
  reappearing.set(recent.path, { at, recent });
  unmarkDeleting(key);
  render();
  void removal
    .then((index) => (index === null ? null : restore(index)))
    .catch((err) => toastError("Could not undo", err))
    // The row stays on screen throughout: it is held here until a list read
    // asked for *after* the restore finished has replaced the cache, so the
    // paint that stops holding it already has the row of its own. A restore
    // that failed drops it too — the toast has said so, and a row for a
    // project that is not there is worse than no row.
    .then(readRecents)
    .catch(() => {})
    .finally(() => {
      reappearing.delete(recent.path);
      render();
    });
}

/**
 * Move a project folder to the Recycle Bin, list entry and all.
 *
 * Nothing is asked first: Undo is the answer to a mis-click, and it is a better
 * one than a dialog in front of every delete. The row goes immediately and
 * comes back if the shell refuses the folder.
 */
function deleteProject(recent: RecentProject): void {
  const key = projectKey(recent.path);
  const at = Math.max(
    knownRecents?.findIndex((row) => row.path === recent.path) ?? 0,
    0,
  );
  markDeleting(key);
  render();

  // Started, not awaited: the toast and the empty row both want to be there
  // before the Recycle Bin has finished thinking about it.
  const deleted = trashProject(recent.path).then(
    (index) => index ?? at,
    (err) => {
      toastError(`Could not delete ${recent.name}`, err);
      return null;
    },
  );
  offerUndo(`Deleted ${recent.name}`, () =>
    undoRemoval(recent, at, key, deleted, (index) =>
      restoreProject(recent.path, index),
    ),
  );
  void deleted.then(() => stopHiding(key));
}

/** Drop a project from the list, leaving the folder where it is. */
function forgetProject(recent: RecentProject): void {
  const key = projectKey(recent.path);
  const at = Math.max(
    knownRecents?.findIndex((row) => row.path === recent.path) ?? 0,
    0,
  );
  markDeleting(key);
  render();

  const forgotten = forgetRecent(recent.path).then(
    (index) => index ?? at,
    (err) => {
      toastError(`Could not forget ${recent.name}`, err);
      return null;
    },
  );
  offerUndo(`Forgot ${recent.name}`, () =>
    undoRemoval(recent, at, key, forgotten, (index) =>
      restoreRecent(recent.path, index),
    ),
  );
  void forgotten.then(() => stopHiding(key));
}

/**
 * Stop hiding a row, once the list itself agrees about it.
 *
 * Dropping the key the moment the call returns is one paint too early:
 * `launchView` draws `knownRecents` before the fresh list has arrived, so a row
 * that is gone from the backend but still in that cache flashes back for the
 * length of one round trip. Reading the list first means the paint that follows
 * has nothing to flash. A row whose delete failed is still in the list and
 * comes back the same way.
 */
function stopHiding(key: string): void {
  // Not worth a toast: the paint below asks for the list again anyway, and the
  // only cost of a failure here is the flicker this avoids.
  void readRecents()
    .catch(() => {})
    .finally(() => {
      unmarkDeleting(key);
      render();
    });
}

function projectCreated(project: Project): void {
  // Already open in Rust, so this records the move and closes the dialog
  // rather than opening the folder a second time. It replaces the dialog's own
  // place instead of following it, so Back goes to the launch screen rather
  // than back into a form whose project already exists.
  state.project = project;
  render();
  void replaceHere({ project: project.root, modal: null });
}

// --- back and forward ------------------------------------------------------
//
// A stack of the app's own places rather than the webview's history, which is
// about URLs this app does not have. A place is "which project, and which
// dialog on top of it", which is the whole of where you can be — so the stack
// runs from the launch screen inwards, and Back and Forward move between
// dialogs as readily as between screens.
//
// It is deliberately not a browser history. Back never retraces a route: it is
// the same move as Escape, always one layer out. What it leaves behind stays
// ahead of the cursor, and Forward is what goes back down to it — to the dialog
// just dismissed, or to the project just left.

/** A dialog, identified by enough to reopen it. */
type Modal =
  | { kind: "entry"; id: string }
  | { kind: "view"; id: string }
  | { kind: "cover" }
  | { kind: "start-date" }
  | { kind: "export" }
  | { kind: "new-project" }
  | { kind: "appearance" };

interface Place {
  /** Project folder, or null for the launch screen. */
  project: string | null;
  modal: Modal | null;
  /** How far down the page was, so coming back lands on the same cards. */
  scroll?: number;
}

/** A fresh launch-screen place; fresh because places are mutated as they are scrolled. */
const launch = (): Place => ({ project: null, modal: null });

const history: Place[] = [launch()];
let cursor = 0;

// The whole stack outlives the page: closing the app, or the dev server
// reloading it after a code change, comes back to the same place with the same
// Back and Forward still available.
const HISTORY_KEY = "journaley.history";

function saveHistory(): void {
  localStorage.setItem(
    HISTORY_KEY,
    JSON.stringify({ places: history, cursor }),
  );
}

/** What the last session saved, or null when there is nothing usable. */
function loadSavedHistory(): { places: Place[]; cursor: number } | null {
  const raw = localStorage.getItem(HISTORY_KEY);
  if (!raw) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;
    const { places, cursor } = parsed as Record<string, unknown>;
    if (!Array.isArray(places) || !places.every(isPlace)) return null;
    if (typeof cursor !== "number" || cursor < 0 || cursor >= places.length) {
      return null;
    }
    return { places, cursor };
  } catch {
    return null;
  }
}

function isPlace(value: unknown): value is Place {
  if (!value || typeof value !== "object") return false;
  const { project, modal, scroll } = value as Record<string, unknown>;
  if (project !== null && typeof project !== "string") return false;
  if (scroll !== undefined && typeof scroll !== "number") return false;
  if (modal === null) return true;
  if (!modal || typeof modal !== "object") return false;
  const { kind, id } = modal as Record<string, unknown>;
  if (kind === "entry" || kind === "view") return typeof id === "string";
  return (
    kind === "cover" ||
    kind === "start-date" ||
    kind === "export" ||
    kind === "new-project" ||
    kind === "appearance"
  );
}

/**
 * How many `apply` calls are in flight, so their own closes and opens are not
 * recorded as moves. A count rather than a flag: `apply` awaits, so a second
 * one can start before the first has finished.
 */
let navigating = 0;

/** Which dialog is on screen, so an unchanged one is not torn down and rebuilt. */
let shownModal: string | null = null;

/** Identifies a dialog well enough to tell "still the same one" from "a different one". */
function modalKey(place: Place): string | null {
  const { project, modal } = place;
  if (!modal) return null;
  // Qualified by the project, so the same kind of dialog over a different
  // project is not mistaken for the one already up.
  const which =
    modal.kind === "entry" || modal.kind === "view"
      ? `${modal.kind}:${modal.id}`
      : modal.kind;
  return `${project}::${which}`;
}

/**
 * Record which entry the viewer is showing, without recording a move.
 *
 * The arrows walk the timeline a picture at a time; a place each would turn
 * Back from a long read into a long walk home. So the current place is edited
 * where it stands, which leaves Back meaning "out of the viewer" throughout.
 */
function noteViewerMoved(id: string): void {
  const place = here();
  if (place.modal?.kind !== "view") return;
  place.modal = { kind: "view", id };
  shownModal = modalKey(place);
  viewerLeftOn = id;
  saveHistory();
}

function here(): Place {
  return history[cursor];
}

/** Go somewhere new. Anything that was ahead of here is dropped, as in a browser. */
async function goTo(place: Place): Promise<void> {
  history.length = cursor + 1;
  history.push(place);
  cursor = history.length - 1;
  saveHistory();
  await apply(place);
}

/**
 * Go somewhere instead of where we are, rather than after it.
 *
 * For a place the one we are on was only ever a step towards: the new-project
 * dialog is replaced by the project it made, so Back from that project reaches
 * the launch screen rather than reopening the form that has already been used.
 */
async function replaceHere(place: Place): Promise<void> {
  history[cursor] = place;
  history.length = cursor + 1;
  saveHistory();
  await apply(place);
}

/** Move the cursor without recording anything, so the way ahead is kept. */
function stepTo(index: number): void {
  cursor = index;
  saveHistory();
  void apply(here());
}

/**
 * Out one layer, and the whole of what Back and Escape both do.
 *
 * Neither retraces where you have been: the stack runs from the launch screen
 * inwards to whatever is on top, and this only ever comes back up it. Going
 * back down is Forward's job.
 */
function goUp(): void {
  // The menu is the topmost layer, and is not itself a place.
  if (isContextMenuOpen()) {
    closeContextMenu();
    return;
  }
  if (isModalOpen()) {
    closeModal();
    return;
  }
  // A field that owns its keystrokes is a layer too: this gets out of the
  // project name before it gets out of the project.
  if (isEditing()) {
    (document.activeElement as HTMLElement | null)?.blur();
    return;
  }
  if (cursor > 0) stepTo(cursor - 1);
}

/** Back down into whatever was last left: the dialog, or the project. */
function goForward(): void {
  closeContextMenu();
  if (cursor < history.length - 1) stepTo(cursor + 1);
}

/** Show a place. False when its project could not be opened. */
async function apply(place: Place): Promise<boolean> {
  navigating += 1;
  try {
    if (place.project !== (state.project?.root ?? null)) {
      if (place.project === null) {
        state.project = null;
        render();
      } else if (!(await loadProject(place.project))) {
        // The folder has been moved or deleted since. Better to stay put than
        // to show a project that is not there.
        return false;
      }
      restoreScroll(place.scroll ?? 0);
    } else {
      // Same page, so it stays where it is — a dialog opening or closing does
      // not move the timeline behind it — and the place records that.
      place.scroll = window.scrollY;
    }

    const wanted = modalKey(place);
    if (wanted === shownModal) return true;
    if (place.modal) {
      showModal(place.modal);
      // An entry deleted since this place was recorded opens nothing.
      shownModal = isModalOpen() ? wanted : null;
    } else {
      closeModal();
      shownModal = null;
      // Every way out of the viewer arrives here, so this is where the page is
      // put back on the entry it ended on. After the scroll was recorded just
      // above, which is the position this replaces.
      landOnViewerEntry();
    }
    return true;
  } finally {
    navigating -= 1;
  }
}

// --- scroll position -------------------------------------------------------
//
// The page scrolls as a whole (`#app` has no scroll container of its own), so
// `scrollY` is the position. It is recorded into the current place as the user
// scrolls, and put back whenever a place is shown again.

/** Undoes the listeners of the restore in progress, if there is one. */
let stopPinning: (() => void) | null = null;

/** Inputs that mean the user has taken the scroll position over. */
const USER_SCROLL_INPUTS = ["wheel", "keydown", "pointerdown", "touchstart"];

/**
 * Scroll to `top`, and keep it there while the cards' images load.
 *
 * A card has no height until its image arrives, so straight after a render the
 * page is shorter than it will be and `top` lands on the wrong cards. Each
 * image that loads pushes everything below it down, so the position is set
 * again on every load until the last one — unless the user scrolls first, at
 * which point it is theirs.
 */
function restoreScroll(top: number): void {
  stopPinning?.();
  window.scrollTo(0, top);

  const pending = Array.from(root.querySelectorAll("img")).filter(
    (image) => !image.complete,
  );
  if (pending.length === 0) return;

  let remaining = pending.length;
  const stop = (): void => {
    stopPinning = null;
    for (const image of pending) {
      image.removeEventListener("load", settle);
      image.removeEventListener("error", settle);
    }
    for (const type of USER_SCROLL_INPUTS) {
      window.removeEventListener(type, stop);
    }
  };
  const settle = (): void => {
    window.scrollTo(0, top);
    if (--remaining === 0) stop();
  };
  for (const image of pending) {
    image.addEventListener("load", settle);
    image.addEventListener("error", settle);
  }
  for (const type of USER_SCROLL_INPUTS) {
    window.addEventListener(type, stop);
  }
  stopPinning = stop;
}

let scrollSave: ReturnType<typeof setTimeout> | null = null;

window.addEventListener("scroll", () => {
  // While a place is being applied the page is mid-rebuild, and where the
  // browser clamps it to says nothing about where the user was.
  if (navigating > 0) return;
  here().scroll = window.scrollY;
  // Scrolling fires every frame; one write once it settles is plenty.
  if (scrollSave !== null) clearTimeout(scrollSave);
  scrollSave = setTimeout(() => {
    scrollSave = null;
    saveHistory();
  }, 200);
});

// The last scroll before the page goes must not wait for the timer.
window.addEventListener("pagehide", saveHistory);

function showModal(modal: Modal): void {
  const project = state.project;
  switch (modal.kind) {
    case "entry":
      openEntryEditor(modal.id, editorContext);
      break;
    case "view":
      viewerLeftOn = modal.id;
      openLightbox(modal.id, viewerContext);
      break;
    case "cover":
      if (project) openCoverPicker(editorContext);
      break;
    case "start-date":
      if (project) openStartDateEditor(project);
      break;
    case "export":
      if (project) openExportDialog(project);
      break;
    case "new-project":
      openNewProjectDialog(projectCreated);
      break;
    case "appearance":
      openAppearanceDialog();
      break;
  }
}

/** Open a dialog over wherever we are. */
function openHere(modal: Modal): void {
  void goTo({ project: here().project, modal });
}

// Dismissing a dialog — the Close button, the backdrop, a delete that closes it
// — is the same move as pressing Back, so Forward brings the dialog back.
onModalDismissed(() => {
  if (navigating > 0 || !here().modal) return;
  // `shownModal` stays as it is: clearing it here would make the step below
  // look like a move to the place already on screen, and `apply` would return
  // without doing the work of leaving — including putting the page back on the
  // entry the viewer ended on.
  const underneath = history[cursor - 1];
  if (
    underneath &&
    underneath.project === here().project &&
    underneath.modal === null
  ) {
    stepTo(cursor - 1);
  } else {
    // Arrived here some other way; the place underneath is not on the stack.
    void goTo({ project: here().project, modal: null });
  }
});

/**
 * The entry the viewer was last showing, until the page has been put back on it.
 *
 * The viewer can walk a long way from the card it was opened on, and every way
 * out of it — Escape, the backdrop, Back, a Forward that steps past it — should
 * leave the page on the entry it ended on rather than where it was left.
 */
let viewerLeftOn: string | null = null;

/** Put the page on the entry the viewer ended on, if it has not been already. */
function landOnViewerEntry(): void {
  const id = viewerLeftOn;
  viewerLeftOn = null;
  if (!id) return;
  const card = root.querySelector(`[data-entry="${CSS.escape(id)}"]`);
  card?.scrollIntoView({ block: "center" });
}

async function addEntry(): Promise<void> {
  try {
    const id = await createEntry();
    // The entry itself arrives via the rescan that the create triggers; open
    // the editor once it has.
    setTimeout(() => openHere({ kind: "entry", id }), 0);
  } catch (err) {
    toastError("Could not add an entry", err);
  }
}

async function runUndo(): Promise<void> {
  // The Rust undo stack belongs to the open project, so on the launch screen
  // the only thing to take back is a project delete or forget.
  if (!state.project) {
    const undo = launchUndo;
    launchUndo = null;
    if (undo) undo();
    else toast("Nothing to undo.");
    return;
  }

  try {
    const outcome = await undoDelete();
    if (!outcome) {
      toast("Nothing to undo.");
      return;
    }
    // The offer this just took up goes, so it cannot be taken twice.
    liveOffers.pop()?.dismiss();
    toast(outcome.message);
  } catch (err) {
    toastError("Could not undo", err);
  }
}

// --- wiring ----------------------------------------------------------------

// A rescan found the folder genuinely different. Redraw, and put any open
// editor back the way it was.
void listen<Project>("project-changed", (event) => {
  state.project = event.payload;
  render();
  const modal = here().modal;
  if (modal?.kind === "entry") refreshEntryEditor(modal.id, editorContext);
  else if (modal?.kind === "view") refreshLightbox(modal.id, viewerContext);
  else if (modal?.kind === "cover") refreshCoverPicker(editorContext);
});

// Files dragged in from Explorer arrive here, not through the DOM drop event.
void getCurrentWebview().onDragDropEvent((event) => {
  if (event.payload.type !== "drop" || !state.project) return;
  const paths = event.payload.paths;
  if (paths.length === 0) return;
  // Into the open entry if there is one, otherwise the cover.
  const modal = here().modal;
  const into =
    modal?.kind === "entry" || modal?.kind === "view" ? modal.id : null;
  void addDroppedPaths(into, paths);
});

// The mouse's thumb buttons. The webview would otherwise take them as history
// navigation and leave the single-page app, so both halves of the click are
// swallowed and turned into a move within the app instead.
const THUMB_BUTTONS = new Map([
  [3, goUp],
  [4, goForward],
]);

for (const type of ["mousedown", "auxclick"]) {
  window.addEventListener(type, (event) => {
    if (THUMB_BUTTONS.has((event as MouseEvent).button)) event.preventDefault();
  });
}

window.addEventListener("mouseup", (event) => {
  const move = THUMB_BUTTONS.get(event.button);
  if (!move) return;
  event.preventDefault();
  move();
});

// A two-finger sideways swipe on a trackpad, which reaches the page as a
// horizontal wheel. Fingers to the right is out one layer and fingers to the
// left is back down, the way every browser reads the same gesture.
//
// One gesture is one move: crossing the threshold makes the move, and the rest
// of that gesture is then ignored. Which needs an answer to where one gesture
// ends, and a pause in the stream is not it — a flick keeps reporting momentum
// for a second and a half, far longer than the pause between two swipes, so a
// second swipe lands in the tail of the first with a gap of 30ms.
//
// Momentum only ever slows down, so the next gesture starts where the wheel
// speeds up again — fingers are back on the glass — or where it turns around,
// which momentum never does either. The quiet gap stays as a backstop.
//
// Speed, not delta: a stalled frame folds several deltas into one fat event,
// and a measured tail ran 12, 11, then 39 after a 67ms gap, which is the same
// speed and no push at all. Divided by the time since the last event, the
// stream is a curve that only falls. Two things still break it:
//
// - A push is not smooth. One ran 16, 63, 66, 82, 232, 0, 1, 42, 31 — two
//   deltas of nothing in the middle of it, then a rise. So a rise is measured
//   against the fastest of the last few events, which the dip does not erase.
// - The move is made on the way up, and the rest of the rise would look like a
//   push. So nothing counts as one until the flick has clearly turned down.
const SWIPE_STEP = 120;
const SWIPE_GAP = 400;
/** How much faster than the recent envelope, in px/ms, fingers have to be. */
const SWIPE_RISE = 0.5;
/** How many recent speeds make up that envelope. */
const SWIPE_WINDOW = 4;
/** The fraction of its peak speed below which a flick has turned down. */
const SWIPE_FALL = 0.6;
/** A delta after a stall is spread over at most this long, ms. */
const SWIPE_STALL = 100;

let swipeTowards = 0;
let lastWheelAt = 0;
let lastDelta = 0;
let swiped = false;
/** Since the move was made: the last few speeds, the fastest, and whether it has turned down. */
let recentSpeeds: number[] = [];
let peakSpeed = 0;
let fallen = false;

window.addEventListener("wheel", (event) => {
  // The viewer claims the up-and-down wheel to walk the timeline, and
  // preventing the default is how it says so — its listener is registered as
  // this module imports it, so it has always run by the time this one does.
  if (event.defaultPrevented) return;

  const delta = event.deltaX;
  const since = event.timeStamp - lastWheelAt;
  if (since > SWIPE_GAP) {
    swipeTowards = 0;
    lastDelta = 0;
    swiped = false;
  }
  lastWheelAt = event.timeStamp;

  // A swipe that is mostly up or down is scrolling the timeline, however
  // crooked it happens to be. A wheel that reports nothing sideways at all
  // lands here too, and leaves the gesture it interrupts as it found it.
  if (Math.abs(delta) <= Math.abs(event.deltaY)) {
    swipeTowards = 0;
    return;
  }

  const reversed = delta * lastDelta < 0;
  lastDelta = delta;
  const speed = Math.abs(delta) / Math.min(Math.max(since, 1), SWIPE_STALL);

  if (swiped) {
    peakSpeed = Math.max(peakSpeed, speed);
    if (speed < SWIPE_FALL * peakSpeed) fallen = true;
    const pushed = fallen && speed > Math.max(...recentSpeeds) + SWIPE_RISE;
    recentSpeeds.push(speed);
    if (recentSpeeds.length > SWIPE_WINDOW) recentSpeeds.shift();
    if (!pushed && !reversed) return;
    swiped = false;
    swipeTowards = 0;
  } else if (reversed) {
    // Turning back mid-gesture starts the count again rather than cancelling
    // out what has already been wound up.
    swipeTowards = 0;
  }

  swipeTowards += delta;
  if (Math.abs(swipeTowards) < SWIPE_STEP) return;

  swiped = true;
  swipeTowards = 0;
  recentSpeeds = [speed];
  peakSpeed = speed;
  fallen = false;
  if (delta > 0) goForward();
  else goUp();
});

window.addEventListener("keydown", (event) => {
  const ctrl = event.ctrlKey || event.metaKey;

  // Alt with the arrows, as a browser has them. The viewer leaves arrows with
  // a modifier alone, so this works from inside it too.
  const arrow = event.key === "ArrowLeft" || event.key === "ArrowRight";
  if (event.altKey && !ctrl && arrow) {
    event.preventDefault();
    if (event.key === "ArrowLeft") goUp();
    else goForward();
    return;
  }

  // Ctrl+Z inside a text field stays the browser's own text undo. Hijacking it
  // globally would make writing a note maddening; the toast keeps the delete
  // undo reachable in that case.
  if (ctrl && event.key === "z" && !event.shiftKey && !isEditing()) {
    event.preventDefault();
    void runUndo();
    return;
  }

  if (event.key === "Escape") {
    event.preventDefault();
    goUp();
  }
});


onDateFormatChange(render);

// After the inline script in `index.html`, which has already put the theme on
// the root element; this adds the accent's derived shades and starts following
// the OS while the choice is `system`.
startTheme();

// Pick up where the last session left off. The first paint is only drawn here
// when it is the launch screen; a project is drawn once it has loaded, rather
// than flashing the launch screen in front of it.
const saved = loadSavedHistory();
if (saved) {
  history.splice(0, history.length, ...saved.places);
  cursor = saved.cursor;
}
if (!here().project) render();

// The other half of the startup timing that `lib.rs` prints to stderr: how long
// the page's own HTML took to arrive, and when the first render happened, both
// counted from the moment the webview began navigating.
if (import.meta.env.DEV) {
  const [navigation] = performance.getEntriesByType("navigation");
  const responseStart =
    navigation instanceof PerformanceNavigationTiming
      ? Math.round(navigation.responseStart)
      : "?";
  console.info(
    `journaley: html arrived at ${responseStart} ms, script ran at ${Math.round(performance.now())} ms`,
  );
}

// `journaley <folder>` opens straight into that project; otherwise the saved
// place is shown again, dialog included.
void startupProject().then(async (path) => {
  if (path) {
    await openRecent(path);
    return;
  }
  if (!(await apply(here()))) {
    // The project folder has gone since last time. Its history is no use
    // without it, so start over from the launch screen.
    history.splice(0, history.length, launch());
    cursor = 0;
    saveHistory();
    render();
  }
});
