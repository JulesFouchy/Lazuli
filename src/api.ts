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
}

/** `Sep 05, 2026`, or `Day 39`. */
export type DateFormat = "real" | "day";

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

export interface RecentProject {
  name: string;
  path: string;
  /** Filename within the project's `cover/`, when one is chosen. */
  cover: string | null;
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
) =>
  invoke<Project>("create_project", {
    parent,
    folder: folderName(name),
    name,
    startDate,
  });

export const closeProject = () => invoke<void>("close_project");

export const recentProjects = () =>
  invoke<RecentProject[]>("recent_projects");

/** Put a project folder at the top of the recents list, without opening it. */
export const addRecent = (path: string) =>
  invoke<void>("add_recent", { path });

/** Drop a project from the recents list, returning where in it the project was. */
export const forgetRecent = (path: string) =>
  invoke<number | null>("forget_recent", { path });

/** Put a forgotten project back at the position it held. */
export const restoreRecent = (path: string, index: number) =>
  invoke<void>("restore_recent", { path, index });

/** Move a project folder to the Recycle Bin, returning its place in recents. */
export const trashProject = (path: string) =>
  invoke<number | null>("trash_project", { path });

/** Take a deleted project back, returning the path it came back at. */
export const restoreProject = (path: string, index: number) =>
  invoke<string>("restore_project", { path, index });

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
