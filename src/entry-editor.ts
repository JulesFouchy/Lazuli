// The entry editor and the cover picker.
//
// There is no save button: edits are written straight to `entry.md`. Every such
// write comes back as a rescan a moment later, so the editor is refreshed in
// place rather than rebuilt — see `refreshEntryEditor`. Rebuilding it would
// replace the note field under the cursor, which is exactly what the user is
// using at the time.

import type { Entry, Project } from "./api";
import { setCover, trashEntry, updateEntry } from "./api";
import { stopCamera } from "./camera";
import { formatRealWorld } from "./dates";
import { renderImagePicker } from "./image-picker";
import {
  markdownInput,
  MARKDOWN_HINT,
  type MarkdownInput,
} from "./md-input";
import { closeModal, openModal, replaceModalBody } from "./modal";
import { entryKey, markDeleting, unmarkDeleting } from "./pending";
import { el, focusWhenActive, toast, toastError } from "./ui";

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
  note: MarkdownInput;
  imagesLabel: HTMLElement;
  pickerHost: HTMLElement;
  /** What the picker was last drawn from, so it is rebuilt only when it changed. */
  pickerSignature: string;
  /** Write the note now rather than when the debounce next fires. */
  flushNote: () => void;
}

let editor: OpenEditor | null = null;

/** The image state the picker reflects, as a single comparable string. */
function pickerSignature(entry: Entry): string {
  return `${entry.image ?? ""}::${entry.images.join("|")}`;
}

export interface EditorContext {
  project: () => Project;
  /** Re-read the entry from the latest project state, or null if it is gone. */
  entry: (id: string) => Entry | null;
  refresh: () => void;
  /**
   * Offer an undo for something just deleted.
   *
   * `deleted` resolves to whether the delete actually happened: the toast goes
   * up before the move into the trash has been asked for.
   */
  noteDeletion: (what: string, deleted: Promise<boolean>) => void;
}

export function openEntryEditor(id: string, context: EditorContext): void {
  const entry = context.entry(id);
  if (!entry) return;

  const built = editorBody(entry, context);
  openModal({
    // No header: the date was in it and in the field at the bottom, and the
    // field is the one that can be edited. Escape, Back and the backdrop
    // close it, and delete is on the card's own right-click menu.
    title: null,
    body: built.node,
    onClose: () => {
      // Escape, the backdrop and Back all land here. Whatever is still sitting
      // in the debounce is written rather than lost.
      editor?.flushNote();
      // Closing the dialog on an open preview leaves nothing to stop it.
      stopCamera();
      editor = null;
    },
  });
  // After `openModal`, which dismisses whatever was there and so clears this.
  editor = built.fields;
  // Straight into the note: writing it is the reason the editor is open.
  focusWhenActive(built.fields.note.node);
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

  if (document.activeElement !== editor.dateInput) {
    editor.dateInput.value = entry.journal_date;
  }
  // Not while it has the caret: what came back is this field's own last save,
  // and anything typed since is newer than it.
  if (document.activeElement !== editor.note.node) {
    editor.note.setValue(entry.text);
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

  let timer: number | undefined;
  const saveNote = async () => {
    window.clearTimeout(timer);
    timer = undefined;
    try {
      await updateEntry(id, { text: note.value() });
    } catch (err) {
      toastError("Could not save the note", err);
    }
  };
  /** Write now if a save is still waiting on the debounce, otherwise do nothing. */
  const flushNote = () => {
    if (timer !== undefined) void saveNote();
  };

  const note = markdownInput({
    class: "input md-input--note",
    placeholder: "What happened today?",
    onInput: () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void saveNote(), SAVE_DEBOUNCE_MS);
    },
    onKeydown: (event) => {
      if (event.key !== "Enter" || event.shiftKey) return;
      // An entry is a sentence, so Enter means "done" and Shift+Enter is the
      // escape hatch for the rare multi-line one — including the one that is a
      // list. Write before closing: the debounce would otherwise still be
      // holding the last few keystrokes.
      event.preventDefault();
      void saveNote().then(() => closeModal());
    },
  });
  note.setValue(entry.text);

  const imagesLabel = el("label", { text: `Images (${entry.images.length})` });
  const pickerHost = el("div", {}, imagePicker(entry, context));

  return {
    node: el(
      "div",
      { class: "modal__body-inner" },
      // Note and images first; the date is already right nearly every time, so
      // it sits at the bottom out of the way rather than in the first field.
      el(
        "div",
        { class: "field" },
        el("label", { text: "Note" }),
        note.node,
        // Said here because the field is Markdown source rather than a
        // formatting toolbar, so nothing else would say it. The field styles
        // what it recognises as you type, which says the rest.
        el("p", { class: "hint", text: MARKDOWN_HINT }),
      ),
      el("div", { class: "field" }, imagesLabel, pickerHost),
      el("div", { class: "field" }, el("label", { text: "Date" }), dateInput),
    ),
    fields: {
      id,
      dateInput,
      note,
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
    onDeleted: (filename, deleted) => context.noteDeletion(filename, deleted),
  });
}

/**
 * Move an entry and its images into the project's trash.
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

  const deleted = trashEntry(id).then(
    () => true,
    (err) => {
      toastError("Could not delete the entry", err);
      return false;
    },
  );
  // Offered at once, like the card disappearing at once.
  // Formatted, not the raw `2026-09-20`: every other date on screen reads as a
  // date, and this one appeared beside them in the trash.
  context.noteDeletion(`the entry from ${formatRealWorld(entry.journal_date)}`, deleted);
  void deleted.then(() => {
    unmarkDeleting(key);
    context.refresh();
  });
}

export function openCoverPicker(context: EditorContext): void {
  openModal({
    title: "Cover image",
    body: coverBody(context),
    // Same as the entry editor: the preview goes off the page with the dialog.
    onClose: stopCamera,
  });
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
      onDeleted: (filename, deleted) => context.noteDeletion(filename, deleted),
    }),
  );
}

/** Shared by both pickers: report what a delete did and how to take it back. */
export function announceDeletion(
  what: string,
  deleted: Promise<boolean>,
  undo: () => void,
  onGone?: () => void,
): () => void {
  return toast(`Deleted ${what}`, {
    action: {
      label: "Undo",
      // The toast goes up before the delete has been asked for, so the undo
      // waits for the answer -- and does nothing if the delete turned out to
      // fail, which would otherwise pop whatever was underneath it.
      run: () => void deleted.then((ok) => ok && undo()),
    },
    onGone,
  });
}
