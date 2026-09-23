// The timeline: entry cards, and the gaps between them.

import type { Entry, Project } from "./api";
import { assetUrl, showSmall } from "./api";
import {
  daysBetween,
  formatDate,
  formatDateAlternate,
  formatGap,
  toggleDateFormat,
} from "./dates";
import { openContextMenu } from "./context-menu";
import { plainText, renderBlocks } from "./markdown";
import { entryKey, isDeleting } from "./pending";
import { authorAvatar } from "./profile";
import { el } from "./ui";


export interface TimelineHandlers {
  /** A click on the picture: the picture, full size. */
  viewEntry: (entry: Entry) => void;
  /** A click anywhere else on the card. */
  editEntry: (entry: Entry) => void;
  deleteEntry: (entry: Entry) => void;
  /** Settle an entry that arrived in more than one version. */
  resolveConflict: (entry: Entry, version: number) => void;
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

/**
 * Who wrote this one, shown only when the project has more than one author.
 *
 * A journal one person keeps is not a list of who did what, and a name on every
 * card of it would be noise the whole way down. The moment somebody else writes
 * here it stops being noise and starts being the point.
 *
 * An entry with no author is one written before authors were recorded, and it
 * is left unlabelled rather than guessed at.
 */
function authorLabel(project: Project, entry: Entry): HTMLElement | false {
  if (Object.keys(project.authors).length < 2) return false;
  const who = entry.author ? project.authors[entry.author] : undefined;
  if (!who) return false;
  return el(
    "span",
    { class: "card__author", title: "Who wrote this" },
    authorAvatar(
      who.avatar &&
        entry.author &&
        assetUrl(project.root, "authors", entry.author, who.avatar),
      who.name,
    ),
    // The name they chose for this project when they chose one, and their own
    // otherwise — see `authors.rs`.
    el("span", { text: who.display_name ?? who.name }),
  );
}

/**
 * The choice an entry that arrived twice has to be settled with.
 *
 * Shown on the card rather than behind a dialog, because an unsettled entry is
 * something to notice while reading the timeline, not something to go looking
 * for. Prose is never merged for you — that is `wip.md`'s own rule — so both
 * versions are here in full and one of them wins.
 */
function conflictChoice(entry: Entry, handlers: TimelineHandlers): HTMLElement {
  const conflict = entry.conflict;
  if (!conflict) return el("div");
  return el(
    "div",
    {
      class: "conflict",
      // The card beneath opens the editor, which is not what any click in here
      // means.
      onclick: (event: Event) => event.stopPropagation(),
    },
    el("p", {
      class: "conflict__why",
      text:
        conflict.kind === "markers"
          ? "This entry came back from a merge in two versions. Keep one."
          : "Two copies of this entry arrived. Keep one.",
    }),
    ...conflict.versions.map((version, index) =>
      el(
        "div",
        { class: "conflict__version" },
        el("div", { class: "conflict__label", text: version.label }),
        el("div", {
          class: version.text ? "conflict__text" : "conflict__text conflict__text--empty",
          text: version.text || "No note",
        }),
        el("button", {
          class: "button",
          text: "Keep this one",
          onclick: () => handlers.resolveConflict(entry, index),
        }),
      ),
    ),
    el("p", {
      class: "hint",
      text: "The one you do not keep goes to the project's trash, not away.",
    }),
  );
}

function entryCard(
  project: Project,
  entry: Entry,
  handlers: TimelineHandlers,
): HTMLElement {
  const card = el(
    "article",
    {
      class: entry.conflict ? "card card--conflicted" : "card",
      // So the viewer can put the page back on whichever card it ended on.
      "data-entry": entry.id,
      // The card edits, the picture views. Everything on a card other than the
      // picture is something you would change rather than look at, so the whole
      // of the rest of it — the date's row, the note, the padding between them
      // — is one target, and only the picture opts out. No tooltip on either:
      // one that follows the pointer across every card on the page is in the
      // way of reading them.
      onclick: () => handlers.editEntry(entry),
      oncontextmenu: (event: Event) =>
        openContextMenu(event as MouseEvent, [
          {
            label: "Delete entry",
            danger: true,
            run: () => handlers.deleteEntry(entry),
          },
        ]),
    },
    // No pencil: the card itself is the edit button, so a second one beside the
    // date was a control that did what clicking next to it already did.
    el(
      "header",
      { class: "card__head" },
      dateToggle(entry),
      authorLabel(project, entry),
    ),
    // A `div` and not a `p`: the note can hold a heading or a list, and a
    // paragraph cannot legally contain either — the browser would close the
    // `p` before them and the card's own text would end up outside it.
    !entry.conflict &&
      el(
        "div",
        { class: entry.text ? "card__text" : "card__text card__text--empty" },
        entry.text ? renderBlocks(entry.text) : "No note yet",
      ),
    // In place of the note, because the whole question is which note this is.
    !!entry.conflict && conflictChoice(entry, handlers),
    // No picture, no frame for one: an entry that is only a sentence is a
    // small card, not a card with a hole in it.
    entry.image !== null &&
      el(
        "div",
        { class: "card__figure" },
        showSmall(
          el("img", {
            class: "card__image",
            // The note without its markers: an `alt` is read aloud, and nobody
            // wants to hear the asterisks.
            alt: plainText(entry.text) || "Entry illustration",
            // Thousands of photos would otherwise all decode at once; the
            // browser skips the offscreen ones.
            loading: "lazy",
            decoding: "async",
            // The one part of a card that is to be looked at rather than
            // changed, so it takes the click back off the card.
            onclick: (event: Event) => {
              event.stopPropagation();
              handlers.viewEntry(entry);
            },
          }) as HTMLImageElement,
          project.root,
          "entries",
          entry.id,
          entry.image,
        ),
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
