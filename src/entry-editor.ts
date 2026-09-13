// The entry editor and the cover picker.
//
// There is no save button: edits are written straight to `entry.md`. Every such
// write comes back as a rescan a moment later, so the editor is refreshed in
// place rather than rebuilt — see `refreshEntryEditor`. Rebuilding it would
// replace the textarea under the cursor, which is exactly what the user is
// using at the time.

import type { Entry, Project } from "./api";
import { setCover, trashEntry, updateEntry } from "./api";
import { onDateFormatChange } from "./dates";
import { renderImagePicker } from "./image-picker";
import { closeModal, openModal, replaceModalBody, setModalTitle } from "./modal";
import { entryKey, markDeleting, unmarkDeleting } from "./pending";
import { dateToggle } from "./timeline";
import { el, toast, toastError } from "./ui";

/** How long to wait after the last keystroke before writing to disk. */
const SAVE_DEBOUNCE_MS = 400;

/**
 * The fields of the editor currently on screen.
 *
 * Held so a rescan can update them individually, leaving whichever one has the
 * cursor alone.
 */
interface OpenEditor {
  id: string;
  dateInput: HTMLInputElement;
  textarea: HTMLTextAreaElement;
  imagesLabel: HTMLElement;
  pickerHost: HTMLElement;
  /** What the picker was last drawn from, so it is rebuilt only when it changed. */
  pickerSignature: string;
  /** Write the note now rather than when the debounce next fires. */
  flushNote: () => void;
}

let editor: OpenEditor | null = null;

/** Drops the open editor's subscription to the global date-format toggle. */
let stopWatchingFormat: (() => void) | null = null;

/** The editor's title: the entry's date, which flips format like any other. */
function titleFor(entry: Entry): HTMLElement {
  return dateToggle(entry, "modal__title date-toggle");
}

/** The image state the picker reflects, as a single comparable string. */
function pickerSignature(entry: Entry): string {
  return `${entry.image ?? ""}::${entry.images.join("|")}`;
}

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

  const built = editorBody(entry, context);
  openModal({
    title: titleFor(entry),
    body: built.node,
    // No delete button: it lives on the card's right-click menu, where it is
    // out of reach of someone who only came here to write a sentence.
    onClose: () => {
      // Escape, the backdrop and Back all land here. Whatever is still sitting
      // in the debounce is written rather than lost.
      editor?.flushNote();
      editor = null;
      stopWatchingFormat?.();
      stopWatchingFormat = null;
    },
  });
  // After `openModal`, which dismisses whatever was there and so clears this.
  editor = built.fields;
  // Nothing rescans when the format flips, so the title has to follow it here.
  stopWatchingFormat = onDateFormatChange(() => {
    const current = context.entry(id);
    if (current) setModalTitle(titleFor(current));
  });
  // Straight into the note: writing it is the reason the editor is open.
  built.fields.textarea.focus();
}

/**
 * Bring the open editor up to date after a rescan.
 *
 * Updates each field rather than replacing the body, and skips whichever field
 * has the cursor: every keystroke in the note is saved, and every save comes
 * back through here, so rebuilding would take the focus away mid-sentence.
 */
export function refreshEntryEditor(id: string, context: EditorContext): void {
  // Not the editor on screen — or none is. Reopening one the user has closed
  // would be worse than doing nothing.
  if (!editor || editor.id !== id) return;

  const entry = context.entry(id);
  if (!entry) {
    replaceModalBody(el("p", { class: "empty", text: "This entry is gone." }));
    editor = null;
    return;
  }

  setModalTitle(titleFor(entry));
  if (document.activeElement !== editor.dateInput) {
    editor.dateInput.value = entry.journal_date;
  }
  if (
    document.activeElement !== editor.textarea &&
    editor.textarea.value !== entry.text
  ) {
    editor.textarea.value = entry.text;
  }

  const signature = pickerSignature(entry);
  if (signature !== editor.pickerSignature) {
    editor.pickerSignature = signature;
    editor.imagesLabel.textContent = `Images (${entry.images.length})`;
    editor.pickerHost.replaceChildren(imagePicker(entry, context));
  }
}

function editorBody(
  entry: Entry,
  context: EditorContext,
): { node: HTMLElement; fields: OpenEditor } {
  const id = entry.id;

  const dateInput = el("input", {
    class: "input input--date",
    type: "date",
    // An entry has a day and no time: this is the whole of its date.
    value: entry.journal_date,
  }) as HTMLInputElement;

  dateInput.addEventListener("change", () => {
    if (!dateInput.value) return;
    void updateEntry(id, { date: dateInput.value }).catch((err) =>
      toastError("Could not change the date", err),
    );
  });

  const textarea = el("textarea", {
    class: "input",
    rows: 3,
    placeholder: "What happened today?",
  }) as HTMLTextAreaElement;
  textarea.value = entry.text;

  let timer: number | undefined;
  const saveNote = async () => {
    window.clearTimeout(timer);
    timer = undefined;
    try {
      await updateEntry(id, { text: textarea.value });
    } catch (err) {
      toastError("Could not save the note", err);
    }
  };
  /** Write now if a save is still waiting on the debounce, otherwise do nothing. */
  const flushNote = () => {
    if (timer !== undefined) void saveNote();
  };
  textarea.addEventListener("input", () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => void saveNote(), SAVE_DEBOUNCE_MS);
  });
  textarea.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || event.shiftKey) return;
    // An entry is a sentence, so Enter means "done" and Shift+Enter is the
    // escape hatch for the rare multi-line one. Write before closing: the
    // debounce would otherwise still be holding the last few keystrokes.
    event.preventDefault();
    void saveNote().then(() => closeModal());
  });

  const imagesLabel = el("label", { text: `Images (${entry.images.length})` });
  const pickerHost = el("div", {}, imagePicker(entry, context));

  return {
    node: el(
      "div",
      { class: "modal__body-inner" },
      // Note and images first; the date is already right nearly every time, so
      // it sits at the bottom out of the way rather than in the first field.
      el("div", { class: "field" }, el("label", { text: "Note" }), textarea),
      el("div", { class: "field" }, imagesLabel, pickerHost),
      el("div", { class: "field" }, el("label", { text: "Date" }), dateInput),
    ),
    fields: {
      id,
      dateInput,
      textarea,
      imagesLabel,
      pickerHost,
      pickerSignature: pickerSignature(entry),
      flushNote,
    },
  };
}

function imagePicker(entry: Entry, context: EditorContext): HTMLElement {
  return renderImagePicker({
    directory: `${context.project().root}/entries/${entry.id}`,
    filenames: entry.images,
    chosen: entry.image,
    entryId: entry.id,
    onChoose: async (filename) => {
      try {
        await updateEntry(entry.id, { image: filename });
        context.refresh();
      } catch (err) {
        toastError("Could not choose that image", err);
      }
    },
    onChanged: () => context.refresh(),
    onDeleted: (filename) => context.noteDeletion(filename),
  });
}

/**
 * Move an entry and its images to the Recycle Bin.
 *
 * Nothing is asked first: the toast offers Undo, and Ctrl+Z reaches the same
 * place, which is a better answer than a dialog in the way of every delete.
 * The card goes at once and comes back if the delete fails.
 */
export function deleteEntry(id: string, context: EditorContext): void {
  const entry = context.entry(id);
  if (!entry) return;

  const key = entryKey(id);
  markDeleting(key);
  closeModal();
  context.refresh();

  void (async () => {
    try {
      await trashEntry(id);
      context.noteDeletion(`the entry from ${entry.journal_date}`);
    } catch (err) {
      toastError("Could not delete the entry", err);
    } finally {
      unmarkDeleting(key);
      context.refresh();
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
