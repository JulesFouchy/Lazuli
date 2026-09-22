// Typed wrappers over the Rust command surface.
//
// The shapes here mirror `src-tauri/src/model.rs`. Rust derives the journal
// date and day number, so the UI never recomputes the 5am rule.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import { plainText } from "./markdown";

export interface ProjectMeta {
  /** A UUID for the project, minted the first time it is opened. */
  id: string | null;
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
  /**
   * Who wrote it, as an author id to look up in the project's `authors`.
   *
   * Null for an entry from before authors were recorded. Shown only when a
   * project has more than one author, so a solo journal reads as it always has.
   */
  author: string | null;
  /**
   * Set when the entry arrived in more than one version and one has to be
   * chosen. Null for every ordinary entry, which is nearly all of them.
   */
  conflict: Conflict | null;
}

/** One version of an entry that arrived in more than one. */
export interface ConflictVersion {
  /** Where this one came from, in words: "Yours", "Theirs", or a filename. */
  label: string;
  text: string;
  /** The day it claims, or null when it does not parse on its own. */
  date: string | null;
  image: string | null;
}

export interface Conflict {
  /** `markers` for a merge's leftovers, `sidecar` for a syncer's second file. */
  kind: "markers" | "sidecar";
  /** The versions to choose between, the file's own first. */
  versions: ConflictVersion[];
}

/**
 * Settle a conflicted entry by keeping the version at `version`.
 *
 * The losers go to the project's trash rather than being removed.
 */
export const resolveConflict = (id: string, version: number) =>
  invoke<void>("resolve_conflict", { id, version });

/** What a project records about one of its authors. */
export interface AuthorProfile {
  name: string;
  /** Picture filename, beside the record in `authors/<id>/`. */
  avatar: string | null;
}

/** This user's own profile, as the avatar button and its dialog show it. */
export interface MyProfile {
  name: string;
  /** A `data:` URL, or null when they have not chosen a picture. */
  avatar: string | null;
}

export const myProfile = () => invoke<MyProfile>("my_profile");

export const setMyName = (name: string) =>
  invoke<MyProfile>("set_my_name", { name });

export const setMyAvatar = (filename: string, bytes: Uint8Array) =>
  invoke<MyProfile>("set_my_avatar", {
    filename,
    bytes: Array.from(bytes),
  });

export const clearMyAvatar = () => invoke<MyProfile>("clear_my_avatar");

export interface Project {
  root: string;
  meta: ProjectMeta;
  /** Oldest first. */
  entries: Entry[];
  cover_images: string[];
  /** Everyone who has written here, by author id. */
  authors: Record<string, AuthorProfile>;
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

/** A project in the projects directory's trash, and where it sat in the list. */
export type TrashedProject = {
  /** The deletion's id, which is what puts it back. */
  id: string;
  slot: Slot;
};

/**
 * Move a project folder into the projects directory's trash.
 *
 * Not the system Recycle Bin: a rename into a folder the app owns behaves the
 * same on every platform, where reading the system bin back does not exist on
 * macOS at all.
 */
export const trashProject = (path: string) =>
  invoke<TrashedProject | null>("trash_project", { path });

/** Take a deleted project back, returning the path it came back at. */
export const restoreProject = (path: string, id: string, slot: Slot) =>
  invoke<string>("restore_project", { path, id, slot });

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
 * What a deleted thing is, named the way the user would name it.
 *
 * An entry's folder is a UUID and says nothing, so an entry carries the day and
 * the sentence instead. The date arrives as the day itself, so the trash can
 * show it the way the project reads its dates.
 */
export type TrashedWhat =
  | { kind: "entry"; date: string; text: string }
  | { kind: "file"; name: string };

/** One thing in a project's `.lazuli-trash/`. */
export type TrashedItem = {
  /** The deletion's id, which is what puts it back. */
  id: string;
  what: TrashedWhat;
  marker: {
    /** Where it was, relative to the project folder. */
    path: string;
    /** When it was deleted, RFC 3339 with the offset it was deleted in. */
    deleted: string;
    /** Which author deleted it, once a project has more than one. */
    by: string | null;
    /** True once the contents have gone on to the system Recycle Bin. */
    purged: boolean;
  };
  /** False once only the record is left and there is nothing to put back. */
  restorable: boolean;
};

/** Everything in the open project's trash, most recently deleted first. */
export const trashContents = () => invoke<TrashedItem[]>("trash_contents");

/**
 * Put one thing back from the trash, rather than from the undo stack.
 *
 * The undo stack only holds what this session did; this reaches anything still
 * in the folder, including a deletion that arrived from another machine.
 */
export const restoreTrashed = (id: string) =>
  invoke<string>("restore_trashed", { id });

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

/** Where a project keeps the downscaled copies of its images. */
const THUMBS_DIR = ".lazuli-thumbs";

/**
 * The downscaled copy of an image, which mirrors its path with a `.jpg` name.
 *
 * Worked out here rather than asked for, because the rule is the whole of it —
 * see `thumbs.rs`. There may be no such file: one is still being built, or the
 * picture is in a format Rust cannot decode. Use [`showSmall`] rather than this
 * directly, so that a card falls back to the picture itself.
 */
export function thumbUrl(root: string, ...segments: string[]): string {
  const path = segments.join("/").replace(/\.[^./]*$/, "");
  return assetUrl(root, THUMBS_DIR, `${path}.jpg`);
}

/**
 * Point an `img` at an image's thumbnail, falling back to the picture itself.
 *
 * The fallback is the browser's own `error` event rather than anything asked of
 * Rust: whether a thumbnail exists is a question with a one-line answer the
 * `img` is already asking, and a command per picture would be a round trip per
 * card.
 */
export function showSmall(
  image: HTMLImageElement,
  root: string,
  ...segments: string[]
): HTMLImageElement {
  const full = assetUrl(root, ...segments);
  image.addEventListener(
    "error",
    () => {
      // Only once: if the original fails too, there is nothing further to try
      // and a loop would be the worst way to find that out.
      if (image.src !== full) image.src = full;
    },
    { once: true },
  );
  image.src = thumbUrl(root, ...segments);
  return image;
}
