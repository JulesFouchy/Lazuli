// The timeline: entry cards, and the gaps between them.

import type { Entry, Project } from "./api";
import { assetUrl } from "./api";
import {
  daysBetween,
  formatDate,
  formatDateAlternate,
  formatGap,
  toggleDateFormat,
} from "./dates";
import { openContextMenu } from "./context-menu";
import { entryKey, isDeleting } from "./pending";
import { el } from "./ui";


export interface TimelineHandlers {
  /** A plain click: the picture, full size. */
  viewEntry: (entry: Entry) => void;
  /** The pencil, or Ctrl+click. */
  editEntry: (entry: Entry) => void;
  deleteEntry: (entry: Entry) => void;
}

/**
 * The entries the timeline draws, oldest first.
 *
 * Entries being deleted are already gone as far as the page is concerned, so
 * the gaps either side of them close up rather than straddling a hole.
 */
export function visibleEntries(project: Project): Entry[] {
  return project.entries.filter((entry) => !isDeleting(entryKey(entry.id)));
}

/** The same entries in the order they appear on the page. */
export function displayedEntries(
  project: Project,
  newestFirst: boolean,
): Entry[] {
  const entries = visibleEntries(project);
  return newestFirst ? [...entries].reverse() : entries;
}

export function renderTimeline(
  project: Project,
  handlers: TimelineHandlers,
  newestFirst: boolean,
): HTMLElement {
  const container = el("div", { class: "timeline__list" });

  const entries = visibleEntries(project);

  if (entries.length === 0) {
    container.append(
      el("p", {
        class: "empty",
        text: "No entries yet. Add the first one to start the timeline.",
      }),
    );
    return container;
  }

  // Always reason about gaps oldest-first, then flip for display, so the
  // arithmetic does not have to care which way the page runs.
  const ordered = entries;
  const pieces: HTMLElement[] = [];

  ordered.forEach((entry, index) => {
    if (index > 0) {
      const previous = ordered[index - 1];
      const elapsed = daysBetween(previous.journal_date, entry.journal_date);
      pieces.push(elapsed === 0 ? stackSpacer() : gapConnector(elapsed));
    }
    pieces.push(entryCard(project, entry, handlers));
  });

  container.append(...(newestFirst ? pieces.reverse() : pieces));
  return container;
}

/** Two entries on the same journal day sit together with no connector. */
function stackSpacer(): HTMLElement {
  return el("div", { class: "stack" });
}

/**
 * The dashed run between two entries, always the same height and always
 * labelled.
 *
 * The length used to grow with the gap, so a long pause took up a lot of the
 * page. The label carries that now — and reads exactly the same for one day as
 * for a hundred, which is what makes two connectors comparable at a glance.
 */
function gapConnector(days: number): HTMLElement {
  return el(
    "div",
    { class: "gap" },
    el("div", { class: "gap__rule" }),
    el("span", { text: formatGap(days) }),
  );
}

function entryCard(
  project: Project,
  entry: Entry,
  handlers: TimelineHandlers,
): HTMLElement {
  const extras = entry.images.length - (entry.image ? 1 : 0);

  const card = el(
    "article",
    {
      class: "card",
      // So the viewer can put the page back on whichever card it ended on.
      "data-entry": entry.id,
      // The card is the picture, so a click opens the picture. Editing is the
      // rarer of the two and has the pencil; Ctrl+click is its shortcut, for
      // the same reason a modifier opens a link in a new tab.
      onclick: (event: Event) => {
        const mouse = event as MouseEvent;
        if (mouse.ctrlKey || mouse.metaKey) handlers.editEntry(entry);
        else handlers.viewEntry(entry);
      },
      oncontextmenu: (event: Event) =>
        openContextMenu(event as MouseEvent, [
          {
            label: "Delete entry",
            danger: true,
            run: () => handlers.deleteEntry(entry),
          },
        ]),
    },
    el(
      "header",
      { class: "card__head" },
      dateToggle(entry),
      el("span", { class: "card__grow" }),
      extras > 0 &&
        el("span", {
          class: "card__count",
          text: `${extras} other ${extras === 1 ? "attempt" : "attempts"}`,
        }),
      el("button", {
        class: "card__edit",
        "aria-label": "Edit entry",
        title: "Edit this entry — or Ctrl+click the card",
        text: "✎",
        onclick: (event: Event) => {
          event.stopPropagation();
          handlers.editEntry(entry);
        },
      }),
    ),
    el(
      "p",
      { class: entry.text ? "card__text" : "card__text card__text--empty" },
      entry.text || "No note yet",
    ),
    el(
      "div",
      { class: "card__figure" },
      entry.image
        ? el("img", {
            class: "card__image",
            src: assetUrl(project.root, "entries", entry.id, entry.image),
            alt: entry.text || "Entry illustration",
            // Thousands of full-resolution photos would otherwise all decode at
            // once; the browser skips the offscreen ones.
            loading: "lazy",
            decoding: "async",
          })
        : el("div", { class: "card__placeholder", text: "No image chosen" }),
    ),
  );

  return card;
}

/**
 * The date label. Clicking it flips every date on the page, so the click must
 * not also open the entry.
 */
export function dateToggle(entry: Entry): HTMLElement {
  return el("button", {
    class: "date-toggle",
    title: `${formatDateAlternate(entry.journal_date, entry.day_number)} — click to switch every date`,
    text: formatDate(entry.journal_date, entry.day_number),
    onclick: (event: Event) => {
      event.stopPropagation();
      toggleDateFormat();
    },
  });
}
