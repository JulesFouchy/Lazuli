// The grid of image candidates shared by the entry editor and the cover
// picker.
//
// Every image ever added is shown. One is marked as chosen; the rest are
// attempts that were kept. Nothing is removed except by an explicit delete.

import { assetUrl, importImageBytes, importImages, trashImage } from "./api";
import { openContextMenu } from "./context-menu";
import { imageKey, isDeleting, markDeleting, unmarkDeleting } from "./pending";
import { el, toast, toastError } from "./ui";

export interface PickerOptions {
  /** Absolute path of the folder the images live in. */
  directory: string;
  filenames: string[];
  chosen: string | null;
  /** Entry id, or null for the project cover. */
  entryId: string | null;
  onChoose: (filename: string | null) => void;
  /** Called after any change that needs the surrounding view redrawn. */
  onChanged: () => void;
  /** Offered as soon as a delete starts; the promise says whether it stuck. */
  onDeleted: (filename: string, deleted: Promise<boolean>) => void;
}

export function renderImagePicker(options: PickerOptions): HTMLElement {
  const grid = el("div", { class: "thumbs" });

  // A thumbnail whose delete is still running is already gone from the page.
  const filenames = options.filenames.filter(
    (filename) => !isDeleting(imageKey(options.directory, filename)),
  );
  for (const filename of filenames) {
    grid.append(thumbnail(filename, options));
  }

  const zone = el(
    "div",
    { class: "dropzone" },
    filenames.length > 0
      ? grid
      : el("p", { class: "hint", text: "No images yet." }),
    el("p", {
      class: "hint",
      text: "Drop images here, or paste one. Every image you add is kept.",
    }),
  );

  wireDrop(zone);
  openPicker = { zone, options };
  return zone;
}

function thumbnail(filename: string, options: PickerOptions): HTMLElement {
  const isChosen = filename === options.chosen;

  return el(
    "button",
    {
      class: isChosen ? "thumb thumb--chosen" : "thumb",
      title: isChosen ? `${filename} (chosen)` : `${filename} — click to choose`,
      onclick: () => options.onChoose(isChosen ? null : filename),
      // Delete is on the menu rather than on a cross in the corner: the cross
      // sat on top of the picture it was about, a few pixels from the click
      // that chooses it.
      oncontextmenu: (event: Event) =>
        openContextMenu(event as MouseEvent, [
          {
            label: "Delete image",
            danger: true,
            run: () => void deleteImage(filename, options),
          },
        ]),
    },
    el("img", {
      src: assetUrl(options.directory, filename),
      alt: filename,
      loading: "lazy",
      decoding: "async",
    }),
    // No badge: the accent outline already says which one is chosen, and a
    // label over the corner of a small square hides part of the picture.
  );
}

async function deleteImage(
  filename: string,
  options: PickerOptions,
): Promise<void> {
  // Gone from the grid at once; back again if the Recycle Bin refuses it.
  const key = imageKey(options.directory, filename);
  markDeleting(key);
  options.onChanged();

  const deleted = trashImage(options.entryId, filename).then(
    () => true,
    (err) => {
      toastError(`Could not delete ${filename}`, err);
      return false;
    },
  );
  // Offered at once, like the thumbnail disappearing at once.
  options.onDeleted(filename, deleted);
  await deleted;
  unmarkDeleting(key);
  options.onChanged();
}

/** Accept files dropped onto the zone. */
function wireDrop(zone: HTMLElement): void {
  zone.addEventListener("dragover", (event) => {
    event.preventDefault();
    zone.classList.add("dropzone--over");
  });
  zone.addEventListener("dragleave", () =>
    zone.classList.remove("dropzone--over"),
  );
  zone.addEventListener("drop", (event) => {
    event.preventDefault();
    zone.classList.remove("dropzone--over");
    // Paths from an Explorer drag arrive through Tauri's own drag-drop event,
    // which the project view forwards; this branch only sees in-page drags.
  });
}

/**
 * The picker on screen, if there is one.
 *
 * Paste is listened for on the document rather than on the zone, because a
 * paste goes to whatever has focus and the zone is a plain `div` that never
 * does. Focus is normally in the note being written beside it, so a zone
 * listener only ever fired when a thumbnail happened to have been clicked
 * first — which is to say, almost never.
 */
let openPicker: { zone: HTMLElement; options: PickerOptions } | null = null;

document.addEventListener("paste", (event) => {
  // Whatever was open last is still the one on screen, unless it has since
  // been taken off it.
  if (!openPicker?.zone.isConnected) return;
  const files = [...(event.clipboardData?.files ?? [])].filter((file) =>
    file.type.startsWith("image/"),
  );
  if (files.length === 0) return;
  // The note beside the picker must keep its own paste; this only claims the
  // event once there is an image in it to claim.
  event.preventDefault();
  for (const file of files) void addPastedFile(file, openPicker.options);
});

/**
 * Chromium's name for a bitmap pasted from the clipboard — a screenshot, or a
 * copy out of another app. A file copied in Explorer arrives under its own
 * name, which is worth keeping.
 */
const UNNAMED = /^image\.[a-z0-9]+$/i;

async function addPastedFile(file: File, options: PickerOptions): Promise<void> {
  const extension = file.type.split("/")[1] ?? "png";
  const stamp = new Date()
    .toISOString()
    .replace(/[:.]/g, "-")
    .slice(0, 19);
  const filename =
    file.name && !UNNAMED.test(file.name)
      ? file.name
      : `pasted-${stamp}.${extension}`;
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const saved = await importImageBytes(options.entryId, filename, bytes);
    toast(`Added ${saved}`);
    options.onChanged();
  } catch (err) {
    toastError("Could not save the pasted image", err);
  }
}

/** Copy files that were dragged in from Explorer. */
export async function addDroppedPaths(
  entryId: string | null,
  paths: string[],
): Promise<void> {
  try {
    const saved = await importImages(entryId, paths);
    if (saved.length === 0) {
      toast("Nothing added: those files are not images Lazuli can show.");
      return;
    }
    // Names can differ from the originals when something already had them.
    const renamed = saved.filter((name, index) => !paths[index]?.endsWith(name));
    toast(
      renamed.length > 0
        ? `Added ${saved.length}, one as ${renamed[0]}`
        : `Added ${saved.length} image${saved.length === 1 ? "" : "s"}`,
    );
  } catch (err) {
    toastError("Could not add those images", err);
  }
}
