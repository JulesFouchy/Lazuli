// App shell: the launch screen, the project view, and the global keys.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import type { Entry, Project, RecentProject } from "./api";
import {
  assetUrl,
  createEntry,
  createProject,
  defaultProjectsDir,
  forgetRecent,
  openProject,
  recentProjects,
  setProjectName,
  startupProject,
  undoDelete,
} from "./api";
import { formatRealWorld, onDateFormatChange } from "./dates";
import {
  announceDeletion,
  openCoverPicker,
  openEntryEditor,
  refreshCoverPicker,
  refreshEntryEditor,
  type EditorContext,
} from "./entry-editor";
import { addDroppedPaths } from "./image-picker";
import { isModalOpen } from "./modal";
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
  /** The entry whose editor is open, so it can be redrawn after a rescan. */
  editingEntryId: string | null;
  coverPickerOpen: boolean;
  newestFirst: boolean;
}

const state: AppState = {
  project: null,
  editingEntryId: null,
  coverPickerOpen: false,
  newestFirst: localStorage.getItem("journaley.newestFirst") !== "false",
};

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
        text: "New project",
        onclick: () => void newProject(),
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

  void recentProjects().then((recents) => {
    if (recents.length === 0) {
      list.replaceWith(
        el("p", { class: "empty", text: "Nothing opened yet." }),
      );
      return;
    }
    list.append(...recents.map(recentRow));
  });

  return view;
}

function recentRow(recent: RecentProject): HTMLElement {
  return el(
    "div",
    { class: "recent__row" },
    el(
      "button",
      {
        class: "recent__grow",
        style: "text-align: left; display: block;",
        onclick: () => void load(recent.path),
      },
      el("div", { class: "recent__name", text: recent.name }),
      el("div", { class: "recent__path", text: recent.path }),
    ),
    el("button", {
      class: "button button--ghost",
      text: "Forget",
      title: "Remove from this list. The folder is left alone.",
      onclick: () => {
        void forgetRecent(recent.path).then(render);
      },
    }),
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
      onclick: () => {
        state.coverPickerOpen = true;
        openCoverPicker(editorContext);
      },
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
    el(
      "div",
      { class: "banner__actions" },
      el("button", {
        class: "button",
        text: "Reveal in Explorer",
        onclick: (event: Event) => {
          event.stopPropagation();
          void revealItemInDir(project.root);
        },
      }),
      el("button", {
        class: "button",
        text: "Close",
        onclick: (event: Event) => {
          event.stopPropagation();
          state.project = null;
          render();
        },
      }),
    ),
    el(
      "div",
      { class: "banner__bar" },
      nameField,
      el("span", {
        class: "banner__meta",
        text: `${project.entries.length} ${
          project.entries.length === 1 ? "entry" : "entries"
        } · started ${formatRealWorld(project.meta.start_date)}`,
      }),
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
        openEntry: (entry: Entry) => {
          state.editingEntryId = entry.id;
          openEntryEditor(entry.id, editorContext);
        },
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
  noteDeletion: (what) => {
    announceDeletion(what, () => void runUndo());
  },
};

// --- actions ---------------------------------------------------------------

async function openFolder(): Promise<void> {
  const chosen = await openDialog({
    directory: true,
    title: "Open a Journaley project",
    defaultPath: await defaultProjectsDir(),
  });
  if (typeof chosen === "string") await load(chosen);
}

async function newProject(): Promise<void> {
  const chosen = await openDialog({
    directory: true,
    title: "Choose an empty folder for the new project",
    // Opens in wherever projects are kept, so the common case is one click.
    defaultPath: await defaultProjectsDir(),
  });
  if (typeof chosen !== "string") return;

  const suggested = chosen.split(/[\\/]/).filter(Boolean).pop() ?? "Project";
  const name = window.prompt("Project name", suggested);
  if (!name) return;

  try {
    state.project = await createProject(chosen, name);
    render();
  } catch (err) {
    toastError("Could not create the project", err);
  }
}

async function load(path: string): Promise<void> {
  try {
    state.project = await openProject(path);
    render();
  } catch (err) {
    toastError("Could not open that folder", err);
  }
}

async function addEntry(): Promise<void> {
  try {
    const id = await createEntry();
    state.editingEntryId = id;
    // The project state arrives via the rescan that the create triggers; open
    // the editor once it has.
    setTimeout(() => openEntryEditor(id, editorContext), 0);
  } catch (err) {
    toastError("Could not add an entry", err);
  }
}

async function runUndo(): Promise<void> {
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
  if (state.editingEntryId && state.project.entries.some((e) => e.id === state.editingEntryId)) {
    refreshEntryEditor(state.editingEntryId, editorContext);
  } else if (state.coverPickerOpen) {
    refreshCoverPicker(editorContext);
  }
});

// Files dragged in from Explorer arrive here, not through the DOM drop event.
void getCurrentWebview().onDragDropEvent((event) => {
  if (event.payload.type !== "drop" || !state.project) return;
  const paths = event.payload.paths;
  if (paths.length === 0) return;
  // Into the open entry if there is one, otherwise the cover.
  void addDroppedPaths(state.editingEntryId, paths);
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

  if (event.key === "Escape" && !isModalOpen()) {
    (document.activeElement as HTMLElement | null)?.blur();
  }
});

// Modals track their own lifetime; clear the flags when they go.
document.addEventListener("click", () => {
  if (!isModalOpen()) {
    state.editingEntryId = null;
    state.coverPickerOpen = false;
  }
});

onDateFormatChange(render);

render();

// `journaley <folder>` opens straight into that project.
void startupProject().then((path) => {
  if (path) void load(path);
});
