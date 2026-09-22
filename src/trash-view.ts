// What is in the project's trash, and putting any of it back.
//
// Ctrl+Z is the answer to a mis-click and reaches only what this session
// deleted. This is the other half: the trash is a folder inside the project, so
// it holds deletions from every session and — once a project is shared — from
// every machine, and any of them can be taken back here.

import type { TrashedItem } from "./api";
import { restoreTrashed, trashContents } from "./api";
import { formatRealWorld } from "./dates";
import { plainText } from "./markdown";
import { openModal, replaceModalBody } from "./modal";
import { el, toastError } from "./ui";

export function openTrashDialog(): void {
  openModal({
    title: "Trash",
    body: el("p", { class: "empty", text: "Reading…" }),
  });
  void refresh();
}

async function refresh(): Promise<void> {
  try {
    replaceModalBody(body(await trashContents()));
  } catch (err) {
    toastError("Could not read the trash", err);
  }
}

function body(items: TrashedItem[]): HTMLElement {
  if (items.length === 0) {
    return el("p", {
      class: "empty",
      text: "Nothing deleted. What you delete waits here for thirty days.",
    });
  }
  return el(
    "div",
    { class: "modal__body-inner" },
    el("p", {
      class: "hint",
      text:
        "Deleted things wait here for thirty days, then go to the Recycle Bin. " +
        "The folder is in your project, so you can also dig through it yourself.",
    }),
    el("ul", { class: "trash" }, ...items.map(row)),
  );
}

function row(item: TrashedItem): HTMLElement {
  return el(
    "li",
    { class: "trash__row" },
    el(
      "div",
      { class: "trash__what" },
      el("span", { class: "trash__name", text: nameOf(item) }),
      el("span", { class: "trash__when", text: describe(item) }),
    ),
    item.restorable
      ? el("button", {
          class: "button button--ghost",
          text: "Put back",
          onclick: () => void putBack(item),
        })
      : // Nothing to offer: the contents are in the Recycle Bin now, and the
        // record left behind is only what stops another machine putting the
        // entry back when it next syncs.
        el("span", { class: "trash__gone", text: "In the Recycle Bin" }),
  );
}

async function putBack(item: TrashedItem): Promise<void> {
  try {
    await restoreTrashed(item.id);
  } catch (err) {
    toastError("Could not put it back", err);
  }
  // Whether or not it worked: a failure usually means it is already gone, and
  // the re-read is what takes the row away.
  await refresh();
}

/**
 * What to call the thing, as the user would.
 *
 * An entry reads as its day and its sentence — its folder is a UUID and would
 * tell them nothing. The date is always the calendar one here, even in a
 * project that reads in day numbers: `Day 39` of a project is a fact about the
 * timeline, and this is a list of things that have left it.
 */
function nameOf(item: TrashedItem): string {
  if (item.what.kind === "file") return item.what.name;
  const day = formatRealWorld(item.what.date);
  const text = plainText(item.what.text).trim();
  return text ? `${day} — ${text}` : day;
}

/**
 * "Deleted 3 days ago", and how long is left.
 *
 * The countdown is the useful half: the date alone does not say when it stops
 * being recoverable here.
 */
function describe(item: TrashedItem): string {
  const days = daysSince(item.marker.deleted);
  const when =
    days <= 0 ? "Deleted today" : days === 1 ? "Deleted yesterday" : `Deleted ${days} days ago`;
  if (!item.restorable) return when;
  const left = 30 - days;
  if (left <= 0) return `${when} · going to the Recycle Bin`;
  if (left === 1) return `${when} · 1 day left`;
  return `${when} · ${left} days left`;
}

function daysSince(when: string): number {
  const then = new Date(when).getTime();
  if (Number.isNaN(then)) return 0;
  return Math.floor((Date.now() - then) / 86_400_000);
}
