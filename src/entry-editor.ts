// The entry editor and the cover picker.
//
// There is no save button: edits are written straight to `entry.md`. The
// watcher sees those writes, rescans, finds nothing new, and stays quiet, so
// the field being typed into is never redrawn underneath the cursor.

import type { Entry, Project } from "./api";
import { setCover, trashEntry, updateEntry } from "./api";
import { formatDate, splitRfc3339, toRfc3339 } from "./dates";
import { renderImagePicker } from "./image-picker";
import { closeModal, openModal, replaceModalBody } from "./modal";
import { el, toast, toastError } from "./ui";

/** How long to wait after the last keystroke before writing to disk. */
const SAVE_DEBOUNCE_MS = 400;

export interface EditorContext {
  project: () => Project;
  /** Re-read the entry from the latest project state, or null if it is gone. */
  entry: (id: string) => Entry | null;
  refresh: () => void;
  noteDeletion: (what: string) => void;
}

export function openEntryEditor(id: string, context: EditorContext): void {
  const entry = context.entry(id);
  if (!entry) return;

  openModal({
    title: formatDate(entry.journal_date, entry.day_number),
    body: editorBody(id, context),
    foot: el(
      "div",
      { class: "modal__foot" },
      el("span", { class: "card__grow" }),
      el("button", {
        class: "button button--danger",
        text: "Delete entry",
        onclick: () => confirmDeleteEntry(id, context),
      }),
    ),
  });
}

/** Redraw the editor in place, e.g. after an image is added or removed. */
export function refreshEntryEditor(id: string, context: EditorContext): void {
  if (context.entry(id)) replaceModalBody(editorBody(id, context));
}

function editorBody(id: string, context: EditorContext): HTMLElement {
  const project = context.project();
  const entry = context.entry(id);
  if (!entry) return el("p", { class: "empty", text: "This entry is gone." });

  const { date, time } = splitRfc3339(entry.created);

  const dateInput = el("input", {
    class: "input",
    type: "date",
    value: date,
  }) as HTMLInputElement;
  const timeInput = el("input", {
    class: "input",
    type: "time",
    value: time,
  }) as HTMLInputElement;

  const saveWhen = async () => {
    if (!dateInput.value || !timeInput.value) return;
    try {
      await updateEntry(id, {
        created: toRfc3339(dateInput.value, timeInput.value),
      });
    } catch (err) {
      toastError("Could not change the date", err);
    }
  };
  dateInput.addEventListener("change", saveWhen);
  timeInput.addEventListener("change", saveWhen);

  const textarea = el("textarea", {
    class: "input",
    rows: 3,
    placeholder: "What happened today?",
  }) as HTMLTextAreaElement;
  textarea.value = entry.text;

  let timer: number | undefined;
  textarea.addEventListener("input", () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(async () => {
      try {
        await updateEntry(id, { text: textarea.value });
      } catch (err) {
        toastError("Could not save the note", err);
      }
    }, SAVE_DEBOUNCE_MS);
  });

  const picker = renderImagePicker({
    directory: `${project.root}/entries/${id}`,
    filenames: entry.images,
    chosen: entry.image,
    entryId: id,
    onChoose: async (filename) => {
      try {
        await updateEntry(id, { image: filename });
        context.refresh();
      } catch (err) {
        toastError("Could not choose that image", err);
      }
    },
    onChanged: () => context.refresh(),
    onDeleted: (filename) => context.noteDeletion(filename),
  });

  return el(
    "div",
    { class: "modal__body-inner" },
    el(
      "div",
      { class: "row" },
      el(
        "div",
        { class: "field" },
        el("label", { text: "Date" }),
        dateInput,
      ),
      el(
        "div",
        { class: "field" },
        el("label", { text: "Time" }),
        timeInput,
      ),
      el(
        "div",
        { class: "field card__grow" },
        el("label", { text: "Journal day" }),
        el("div", {
          class: "input",
          // The wall-clock time and the journal day are both shown, so an
          // entry written at 01:00 filing under the previous day is visible
          // rather than mysterious.
          text: `Day ${entry.day_number} · ${entry.journal_date}`,
        }),
      ),
    ),
    el(
      "div",
      { class: "field" },
      el("label", { text: "Note" }),
      textarea,
    ),
    el(
      "div",
      { class: "field" },
      el("label", { text: `Images (${entry.images.length})` }),
      picker,
    ),
  );
}

function confirmDeleteEntry(id: string, context: EditorContext): void {
  const entry = context.entry(id);
  if (!entry) return;

  const count = entry.images.length;
  const what =
    count === 0
      ? "this entry"
      : `this entry and its ${count} image${count === 1 ? "" : "s"}`;

  // The only action that discards more than is visible on screen, so this one
  // asks first. Image deletes do not.
  if (!window.confirm(`Move ${what} to the Recycle Bin?`)) return;

  void (async () => {
    try {
      await trashEntry(id);
      context.noteDeletion(`the entry from ${entry.journal_date}`);
      closeModal();
      context.refresh();
    } catch (err) {
      toastError("Could not delete the entry", err);
    }
  })();
}

export function openCoverPicker(context: EditorContext): void {
  openModal({ title: "Cover image", body: coverBody(context) });
}

export function refreshCoverPicker(context: EditorContext): void {
  replaceModalBody(coverBody(context));
}

function coverBody(context: EditorContext): HTMLElement {
  const project = context.project();
  return el(
    "div",
    {},
    renderImagePicker({
      directory: `${project.root}/cover`,
      filenames: project.cover_images,
      chosen: project.meta.cover,
      entryId: null,
      onChoose: async (filename) => {
        try {
          await setCover(filename);
          context.refresh();
        } catch (err) {
          toastError("Could not set the cover", err);
        }
      },
      onChanged: () => context.refresh(),
      onDeleted: (filename) => context.noteDeletion(filename),
    }),
  );
}

/** Shared by both pickers: report what a delete did and how to take it back. */
export function announceDeletion(what: string, undo: () => void): void {
  toast(`Deleted ${what}`, { action: { label: "Undo", run: undo } });
}
