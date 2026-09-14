// The fullscreen entry viewer.
//
// A card crops its picture to keep several entries on the page at once; this is
// the one place the picture is shown whole. Left and right walk the timeline
// without leaving the viewer, so a project can be read straight through.

import type { Entry, Project } from "./api";
import { assetUrl } from "./api";
import { onDateFormatChange } from "./dates";
import { closeModal, openModal } from "./modal";
import { dateToggle } from "./timeline";
import { el } from "./ui";

export interface ViewerContext {
  project: () => Project;
  /** Re-read the entry from the latest project state, or null if it is gone. */
  entry: (id: string) => Entry | null;
  /** The entries in the order the page shows them. */
  entries: () => Entry[];
  /** Open the editor for this entry. */
  edit: (id: string) => void;
  /**
   * The viewer is showing a different entry.
   *
   * Walking the timeline is not a move: it records which entry is on screen
   * without pushing a place, so Back leaves the viewer rather than retracing
   * every arrow press one picture at a time.
   */
  moved: (id: string) => void;
}

interface OpenViewer {
  id: string;
  context: ViewerContext;
  stage: HTMLElement;
  caption: HTMLElement;
  prev: HTMLButtonElement;
  next: HTMLButtonElement;
}

let viewer: OpenViewer | null = null;

export function openLightbox(id: string, context: ViewerContext): void {
  if (!context.entry(id)) return;

  const stage = el("div", { class: "viewer__stage" });
  const caption = el("div", { class: "viewer__caption" });

  const body = el(
    "div",
    {
      class: "viewer",
      // The viewer fills the overlay, so what looks like backdrop around the
      // picture is really the viewer's own empty space. Clicking it has to
      // dismiss, or the one dialog that covers the window would be the one
      // that cannot be clicked away.
      onclick: (event: Event) => {
        if (event.target === body || event.target === stage) closeModal();
      },
    },
    stage,
    caption,
    navButton("prev", "‹", "Previous entry (←)"),
    navButton("next", "›", "Next entry (→)"),
  );

  openModal({
    // No header: the date is in the caption, under the picture it belongs to.
    title: null,
    class: "modal--viewer",
    body,
    onClose: () => {
      viewer = null;
    },
  });

  // After `openModal`, which dismisses whatever was there and so clears this.
  viewer = {
    id,
    context,
    stage,
    caption,
    prev: body.querySelector(".viewer__nav--prev") as HTMLButtonElement,
    next: body.querySelector(".viewer__nav--next") as HTMLButtonElement,
  };
  draw();
}

/** Bring the viewer up to date after a rescan. */
export function refreshLightbox(id: string, context: ViewerContext): void {
  // Not the entry on screen — or the viewer is not open. Either way there is
  // nothing here to correct.
  if (!viewer || viewer.id !== id) return;
  viewer.context = context;
  draw();
}

function navButton(
  side: "prev" | "next",
  glyph: string,
  title: string,
): HTMLElement {
  return el("button", {
    class: `viewer__nav viewer__nav--${side}`,
    title,
    "aria-label": title,
    text: glyph,
    onclick: (event: Event) => {
      event.stopPropagation();
      step(side === "prev" ? -1 : 1);
    },
  });
}

/** Move `delta` entries along the page's own order, if there is one to move to. */
function step(delta: number): void {
  if (!viewer) return;
  const entries = viewer.context.entries();
  const at = entries.findIndex((entry) => entry.id === viewer?.id);
  const target = at < 0 ? undefined : entries[at + delta];
  if (!target) return;
  viewer.id = target.id;
  viewer.context.moved(target.id);
  draw();
}

function draw(): void {
  if (!viewer) return;
  const { context, stage, caption } = viewer;
  const entry = context.entry(viewer.id);

  if (!entry) {
    stage.replaceChildren(el("p", { class: "empty", text: "This entry is gone." }));
    caption.replaceChildren();
    viewer.prev.disabled = true;
    viewer.next.disabled = true;
    return;
  }

  stage.replaceChildren(
    entry.image
      ? el("img", {
          class: "viewer__image",
          src: assetUrl(context.project().root, "entries", entry.id, entry.image),
          alt: entry.text || "Entry illustration",
        })
      : el("div", { class: "viewer__placeholder", text: "No image chosen" }),
  );

  caption.replaceChildren(
    dateToggle(entry),
    el(
      "p",
      { class: entry.text ? "viewer__text" : "viewer__text viewer__text--empty" },
      entry.text || "No note yet",
    ),
    el("button", {
      class: "viewer__edit",
      title: "Edit this entry",
      text: "✎",
      "aria-label": "Edit entry",
      onclick: (event: Event) => {
        event.stopPropagation();
        context.edit(entry.id);
      },
    }),
  );

  const entries = context.entries();
  const at = entries.findIndex((other) => other.id === entry.id);
  viewer.prev.disabled = at <= 0;
  viewer.next.disabled = at < 0 || at >= entries.length - 1;
}

// The caption's date is a toggle like any other, and flips the whole page. The
// page behind redraws itself; the caption has to follow, or the one date the
// user actually clicked is the one that does not change.
onDateFormatChange(() => {
  if (viewer) draw();
});

// The arrows, for as long as the viewer is up. Registered once rather than
// added and removed around each open: the viewer is the only thing that wants
// them, and a stale listener is one more thing to get wrong.
window.addEventListener("keydown", (event) => {
  if (!viewer || event.ctrlKey || event.metaKey || event.altKey) return;
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  step(event.key === "ArrowLeft" ? -1 : 1);
});
