// App shell: the launch screen, the project view, and the global keys.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

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
import { closeModal, isModalOpen, onModalDismissed } from "./modal";
import { isDeleting, markDeleting, projectKey, unmarkDeleting } from "./pending";
import { openNewProjectDialog, openStartDateEditor } from "./project-setup";
import { renderTimeline } from "./timeline";
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
    ),
    el("div", { class: "launch__heading", text: "Recent" }),
  );

  const list = el("div", { class: "recent" });
  view.append(list);

  const paint = (recents: RecentProject[]) => {
    const rows = recents.filter(
      (recent) => !isDeleting(projectKey(recent.path)),
    );
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
  void recentProjects().then((recents) => {
    knownRecents = recents;
    paint(recents);
  });

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
      class: "recent__row",
      onclick: () => void openRecent(recent.path),
      oncontextmenu: (event: Event) =>
        openContextMenu(event as MouseEvent, [
          {
            label: "Reveal in Explorer",
            run: () => void revealItemInDir(recent.path),
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
        })
      : null,
    el("div", { class: "recent__scrim" }),
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
      class: "banner",
      title: "Click to change the cover",
      onclick: () => openHere({ kind: "cover" }),
    },
    cover
      ? el("img", {
          class: "banner__image",
          src: assetUrl(project.root, "cover", cover),
          alt: `${project.meta.name} cover`,
        })
      : null,
    el("div", { class: "banner__scrim" }),
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
        openEntry: (entry: Entry) => openHere({ kind: "entry", id: entry.id }),
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
    announceDeletion(what, deleted, () => void runUndo());
  },
};

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

/** Open a project folder, reporting whether it worked. */
async function loadProject(path: string): Promise<boolean> {
  try {
    state.project = await openProject(path);
    render();
    return true;
  } catch (err) {
    toastError("Could not open that folder", err);
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
  launchUndo = undo;
  toast(message, {
    action: {
      label: "Undo",
      run: () => {
        launchUndo = null;
        undo();
      },
    },
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
  markDeleting(key);
  render();

  // Started, not awaited: the toast and the empty row both want to be there
  // before the Recycle Bin has finished thinking about it.
  const deleted = trashProject(recent.path).then(
    (index) => index ?? 0,
    (err) => {
      toastError(`Could not delete ${recent.name}`, err);
      return null;
    },
  );
  offerUndo(`Deleted ${recent.name}`, () => {
    void deleted.then((index) => {
      if (index === null) return;
      void restoreProject(recent.path, index)
        .then(render)
        .catch((err) => toastError("Could not undo", err));
    });
  });
  void deleted.then(() => {
    unmarkDeleting(key);
    render();
  });
}

/** Drop a project from the list, leaving the folder where it is. */
function forgetProject(recent: RecentProject): void {
  const key = projectKey(recent.path);
  markDeleting(key);
  render();

  const forgotten = forgetRecent(recent.path).then(
    (index) => index ?? 0,
    (err) => {
      toastError(`Could not forget ${recent.name}`, err);
      return null;
    },
  );
  offerUndo(`Forgot ${recent.name}`, () => {
    void forgotten.then((index) => {
      if (index === null) return;
      void restoreRecent(recent.path, index)
        .then(render)
        .catch((err) => toastError("Could not undo", err));
    });
  });
  void forgotten.then(() => {
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
// A browser-shaped history over the app's own places rather than the webview's,
// which is about URLs this app does not have. A place is "which project, and
// which dialog on top of it", which is the whole of where you can be — so Back
// and Forward move between dialogs as readily as between screens.

/** A dialog, identified by enough to reopen it. */
type Modal =
  | { kind: "entry"; id: string }
  | { kind: "cover" }
  | { kind: "start-date" }
  | { kind: "export" }
  | { kind: "new-project" };

interface Place {
  /** Project folder, or null for the launch screen. */
  project: string | null;
  modal: Modal | null;
}

const history: Place[] = [{ project: null, modal: null }];
let cursor = 0;

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
  return `${project}::${modal.kind === "entry" ? `entry:${modal.id}` : modal.kind}`;
}

function here(): Place {
  return history[cursor];
}

/** Go somewhere new. Anything that was ahead of here is dropped, as in a browser. */
async function goTo(place: Place): Promise<void> {
  history.length = cursor + 1;
  history.push(place);
  cursor = history.length - 1;
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
  await apply(place);
}

/** Move the cursor without recording anything, so the way ahead is kept. */
function stepTo(index: number): void {
  cursor = index;
  void apply(here());
}

function goBack(): void {
  // The menu is the topmost layer, and is not itself a place.
  if (isContextMenuOpen()) {
    closeContextMenu();
    return;
  }
  if (cursor > 0) stepTo(cursor - 1);
}

function goForward(): void {
  closeContextMenu();
  if (cursor < history.length - 1) stepTo(cursor + 1);
}

async function apply(place: Place): Promise<void> {
  navigating += 1;
  try {
    if (place.project !== (state.project?.root ?? null)) {
      if (place.project === null) {
        state.project = null;
        render();
      } else if (!(await loadProject(place.project))) {
        // The folder has been moved or deleted since. Better to stay put than
        // to show a project that is not there.
        return;
      }
    }

    const wanted = modalKey(place);
    if (wanted === shownModal) return;
    if (place.modal) {
      showModal(place.modal);
      // An entry deleted since this place was recorded opens nothing.
      shownModal = isModalOpen() ? wanted : null;
    } else {
      closeModal();
      shownModal = null;
    }
  } finally {
    navigating -= 1;
  }
}

function showModal(modal: Modal): void {
  const project = state.project;
  switch (modal.kind) {
    case "entry":
      openEntryEditor(modal.id, editorContext);
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
  shownModal = null;
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
  else if (modal?.kind === "cover") refreshCoverPicker(editorContext);
});

// Files dragged in from Explorer arrive here, not through the DOM drop event.
void getCurrentWebview().onDragDropEvent((event) => {
  if (event.payload.type !== "drop" || !state.project) return;
  const paths = event.payload.paths;
  if (paths.length === 0) return;
  // Into the open entry if there is one, otherwise the cover.
  const modal = here().modal;
  void addDroppedPaths(modal?.kind === "entry" ? modal.id : null, paths);
});

// The mouse's thumb buttons. The webview would otherwise take them as history
// navigation and leave the single-page app, so both halves of the click are
// swallowed and turned into a move within the app instead.
const THUMB_BUTTONS = new Map([
  [3, goBack],
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

window.addEventListener("keydown", (event) => {
  const ctrl = event.ctrlKey || event.metaKey;

  // Ctrl+Z inside a text field stays the browser's own text undo. Hijacking it
  // globally would make writing a note maddening; the toast keeps the delete
  // undo reachable in that case.
  if (ctrl && event.key === "z" && !event.shiftKey && !isEditing()) {
    event.preventDefault();
    void runUndo();
    return;
  }

  // Escape peels off one layer at a time: the menu, then the dialog, then
  // whatever has the cursor.
  if (event.key === "Escape") {
    event.preventDefault();
    if (isContextMenuOpen()) closeContextMenu();
    else if (isModalOpen()) closeModal();
    // A field that owns its keystrokes is a layer too: Escape gets out of the
    // project name before it gets out of the project.
    else if (isEditing()) (document.activeElement as HTMLElement | null)?.blur();
    else if (state.project) void goTo({ project: null, modal: null });
  }
});


onDateFormatChange(render);

render();

// `journaley <folder>` opens straight into that project.
void startupProject().then((path) => {
  if (path) void openRecent(path);
});
