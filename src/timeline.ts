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

/** Pixels of dashed rule per elapsed day, and the ceiling on that. */
const GAP_BASE = 22;
const GAP_PER_DAY = 6;
const GAP_MAX = 200;

export interface TimelineHandlers {
  openEntry: (entry: Entry) => void;
  deleteEntry: (entry: Entry) => void;
}

export function renderTimeline(
  project: Project,
  handlers: TimelineHandlers,
  newestFirst: boolean,
): HTMLElement {
  const container = el("div", { class: "timeline__list" });

  // Entries being deleted are already gone as far as the page is concerned, so
  // the gaps either side of them close up rather than straddling a hole.
  const entries = project.entries.filter(
    (entry) => !isDeleting(entryKey(entry.id)),
  );

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

function gapConnector(days: number): HTMLElement {
  const height = Math.min(GAP_BASE + GAP_PER_DAY * days, GAP_MAX);
  return el(
    "div",
    { class: days === 1 ? "gap gap--tight" : "gap" },
    el("div", { class: "gap__rule", style: `height: ${height}px` }),
    // A single day between entries is the normal case and needs no label; the
    // small dash speaks for itself.
    days === 1 ? null : el("span", { text: formatGap(days) }),
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
      onclick: () => handlers.openEntry(entry),
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
    ),
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
    el(
      "p",
      { class: entry.text ? "card__text" : "card__text card__text--empty" },
      entry.text || "No note yet",
    ),
  );

  return card;
}

/**
 * The date label. Clicking it flips every date on the page, so the click must
 * not also open the entry.
 */
function dateToggle(entry: Entry): HTMLElement {
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
