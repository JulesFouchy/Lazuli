// Typed wrappers over the Rust command surface.
//
// The shapes here mirror `src-tauri/src/model.rs`. Rust derives the journal
// date and day number, so the UI never recomputes the 5am rule.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import { plainText } from "./markdown";

export interface ProjectMeta {
  name: string;
  /** `YYYY-MM-DD`. A journal date: Day 1. */
  start_date: string;
  /** Filename within `cover/`, or null. */
  cover: string | null;
  /** How this project's dates are read. A property of the project, not of the machine. */
  date_format: DateFormat;
  /** Which end of the timeline comes first. A property of the project, not of the machine. */
  sort_order: SortOrder;
}

/** `Sep 05, 2026`, or `Day 39`. */
export type DateFormat = "real" | "day";

/** The most recent entry at the top, or day one at the top. */
export type SortOrder = "newest" | "oldest";

export interface Entry {
  /** The entry folder's name. Stable across date edits. */
  id: string;
  text: string;
  /** The chosen image, or null when none is set or the file has gone. */
  image: string | null;
  /** Every image in the entry folder, sorted. */
  images: string[];
  /**
   * `YYYY-MM-DD`: the day the entry is about, and the only date it has.
   *
   * An entry records no time. Rust keeps a `created` stamp in the file to order
   * entries that share a day, and deliberately does not send it here.
   */
  journal_date: string;
  day_number: number;
}

export interface Project {
  root: string;
  meta: ProjectMeta;
  /** Oldest first. */
  entries: Entry[];
  cover_images: string[];
}

/** A project as the launch screen lists it, without opening it. */
export interface ListedProject {
  name: string;
  path: string;
  /** Filename within the project's `cover/`, when one is chosen. */
  cover: string | null;
}

/** One tab of the launch screen, and what is filed under it. */
export interface ProjectTab {
  name: string;
  projects: ListedProject[];
}

/**
 * A tab as it is stored, holding paths rather than projects.
 *
 * What deleting a tab hands back, and the only thing needed to put it back:
 * the rows are read from the folders again, as they are for every other paint.
 */
export interface StoredTab {
  name: string;
  projects: string[];
}

/** Where a project sits: which tab, and where in it. */
export interface Slot {
  tab: number;
  index: number;
}

export interface UndoOutcome {
  message: string;
  more: boolean;
}

export const openProject = (path: string) =>
  invoke<Project>("open_project", { path });

export interface NewProjectTarget {
  /** The folder that would be created: `parent` plus the project's `folderName`. */
  path: string;
  /** Why that folder cannot be used, or null when it can. */
  problem: string | null;
}

/**
 * What a project called `name` should have as its folder.
 *
 * A name is Markdown, and a folder should be called what the name *reads* as:
 * `**Test** Test` belongs in `Test Test`, not in a folder with the asterisks
 * still in it. Rust cannot work this out — it knows nothing about Markdown — so
 * every command that touches the folder is told, and it is worked out here
 * rather than at each call so that none of them can forget.
 *
 * Rust still sanitises whatever it is given for the filesystem.
 */
const folderName = (name: string): string => plainText(name);

/** Preview the folder a new project would land in, without creating anything. */
export const newProjectTarget = (parent: string, name: string) =>
  invoke<NewProjectTarget>("new_project_target", {
    parent,
    folder: folderName(name),
  });

/** Create a project in a new folder inside `parent`. `startDate` is `YYYY-MM-DD`. */
export const createProject = (
  parent: string,
  name: string,
  startDate: string,
  tab: number,
) =>
  invoke<Project>("create_project", {
    parent,
    folder: folderName(name),
    name,
    startDate,
    tab,
  });

export const closeProject = () => invoke<void>("close_project");

/** Every project the app knows about, in the tabs they are filed under. */
export const projectTabs = () => invoke<ProjectTab[]>("project_tabs");

/** File a project folder at a slot in the list, without opening it. */
export const addProject = (path: string, slot: Slot) =>
  invoke<void>("add_project", { path, slot });

/** Put a project at a slot: the far end of a drag, within a tab or across two. */
export const moveProject = (path: string, slot: Slot) =>
  invoke<void>("move_project", { path, slot });

/** Drop a project from the list, returning the slot it held. */
export const forgetProject = (path: string) =>
  invoke<Slot | null>("forget_project", { path });

/** Put a forgotten project back at the slot it held. */
export const restoreListing = (path: string, slot: Slot) =>
  invoke<void>("restore_listing", { path, slot });

/** Move a project folder to the Recycle Bin, returning the slot it held. */
export const trashProject = (path: string) =>
  invoke<Slot | null>("trash_project", { path });

/** Take a deleted project back, returning the path it came back at. */
export const restoreProject = (path: string, slot: Slot) =>
  invoke<string>("restore_project", { path, slot });

/** Add a tab at the end of the strip, returning where it landed. */
export const addTab = (name: string) => invoke<number>("add_tab", { name });

export const renameTab = (index: number, name: string) =>
  invoke<void>("rename_tab", { index, name });

/**
 * Remove a tab, handing whatever was in it to the first tab.
 *
 * Returns the tab as it was, which is what puts it back. Nothing on disk moves
 * either way: a tab is filing and only filing.
 */
export const deleteTab = (index: number) =>
  invoke<StoredTab | null>("delete_tab", { index });

/** Put a deleted tab back where it was, projects and all. */
export const restoreTab = (index: number, tab: StoredTab) =>
  invoke<void>("restore_tab", { index, tab });

export const moveTab = (from: number, to: number) =>
  invoke<void>("move_tab", { from, to });

/**
 * Rename the project, and the folder it lives in if the folder was named after
 * it.
 *
 * `wasCalled` is the name being replaced. Rust needs it to tell a folder it
 * named itself from one the user named — and needs it as a *folder* name, for
 * the same reason the new one is a folder name.
 */
export const setProjectName = (name: string, wasCalled: string) =>
  invoke<void>("set_project_name", {
    name,
    folder: folderName(name),
    wasFolder: folderName(wasCalled),
  });

export const setStartDate = (startDate: string) =>
  invoke<void>("set_start_date", { startDate });

export const setCover = (filename: string | null) =>
  invoke<void>("set_cover", { filename });

export const setDateFormat = (format: DateFormat) =>
  invoke<void>("set_date_format", { format });

export const setSortOrder = (order: SortOrder) =>
  invoke<void>("set_sort_order", { order });

/**
 * Open the webview's context menu on the word the caret is in, which is where
 * the spelling suggestions are. Rust presses the Menu key; see `keys.rs`.
 */
export const showSpellingSuggestions = () =>
  invoke<void>("show_spelling_suggestions");

/** Add an entry. `date` is `YYYY-MM-DD`, defaulting to the journal day in progress. */
export const createEntry = (date?: string) =>
  invoke<string>("create_entry", { date: date ?? null });

/**
 * Patch an entry. Omitted fields are left alone; passing `image: null` clears
 * the chosen image, whereas leaving `image` out keeps it.
 */
export const updateEntry = (
  id: string,
  patch: { date?: string; text?: string; image?: string | null },
) =>
  invoke<void>("update_entry", {
    id,
    date: patch.date ?? null,
    text: patch.text ?? null,
    // The Rust side takes `Option<Option<String>>`: absent means "no change",
    // present-but-null means "clear". `undefined` serialises to absent.
    image: "image" in patch ? patch.image : undefined,
  });

export const importImages = (entryId: string | null, sources: string[]) =>
  invoke<string[]>("import_images", { entryId, sources });

export const importImageBytes = (
  entryId: string | null,
  filename: string,
  bytes: Uint8Array,
) =>
  invoke<string>("import_image_bytes", {
    entryId,
    filename,
    bytes: Array.from(bytes),
  });

export const trashImage = (entryId: string | null, filename: string) =>
  invoke<void>("trash_image", { entryId, filename });

export const trashEntry = (id: string) => invoke<void>("trash_entry", { id });

export const undoDelete = () => invoke<UndoOutcome | null>("undo_delete");

/**
 * Mirror the theme choice into the Rust settings file.
 *
 * The page's own copy is in `localStorage`, which is the one that matters for
 * painting the page. This copy exists so the *window* can be built in the right
 * theme next launch, before any of this is running — see `theme.rs`.
 */
export const setThemePreference = (
  theme: string,
  backgroundDark: string,
  backgroundLight: string,
) =>
  invoke<void>("set_theme_preference", {
    theme,
    backgroundDark,
    backgroundLight,
  });

/** Where the "new project" dialog should open. */
export const defaultProjectsDir = () => invoke<string>("default_projects_dir");

/** A project folder named on the command line, if any. */
export const startupProject = () => invoke<string | null>("startup_project");

/** The journal day currently in progress, `YYYY-MM-DD`. */
export const journalToday = () => invoke<string>("journal_today");

/** A `file://` path the webview is allowed to load. */
export function assetUrl(...segments: string[]): string {
  return convertFileSrc(segments.join("/"));
}
