// Typed wrappers over the Rust command surface.
//
// The shapes here mirror `src-tauri/src/model.rs`. Rust derives the journal
// date and day number, so the UI never recomputes the 5am rule.

import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";

export interface ProjectMeta {
  name: string;
  /** `YYYY-MM-DD`. A journal date: Day 1. */
  start_date: string;
  /** Filename within `cover/`, or null. */
  cover: string | null;
}

export interface Entry {
  /** The entry folder's name. Stable across date edits. */
  id: string;
  /** RFC 3339 with an offset. */
  created: string;
  text: string;
  /** The chosen image, or null when none is set or the file has gone. */
  image: string | null;
  /** Every image in the entry folder, sorted. */
  images: string[];
  /** `YYYY-MM-DD`, already shifted by the 5am rule. */
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
}

export interface UndoOutcome {
  message: string;
  more: boolean;
}

export const openProject = (path: string) =>
  invoke<Project>("open_project", { path });

export const createProject = (path: string, name: string) =>
  invoke<Project>("create_project", { path, name });

export const closeProject = () => invoke<void>("close_project");

export const recentProjects = () =>
  invoke<RecentProject[]>("recent_projects");

export const forgetRecent = (path: string) =>
  invoke<void>("forget_recent", { path });

export const setProjectName = (name: string) =>
  invoke<void>("set_project_name", { name });

export const setStartDate = (startDate: string) =>
  invoke<void>("set_start_date", { startDate });

export const setCover = (filename: string | null) =>
  invoke<void>("set_cover", { filename });

export const createEntry = (created?: string) =>
  invoke<string>("create_entry", { created: created ?? null });

/**
 * Patch an entry. Omitted fields are left alone; passing `image: null` clears
 * the chosen image, whereas leaving `image` out keeps it.
 */
export const updateEntry = (
  id: string,
  patch: { created?: string; text?: string; image?: string | null },
) =>
  invoke<void>("update_entry", {
    id,
    created: patch.created ?? null,
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
