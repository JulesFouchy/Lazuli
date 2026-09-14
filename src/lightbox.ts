// The fullscreen entry viewer.
//
// A card crops its picture to keep several entries on the page at once; this is
// the one place the picture is shown whole, and the one place it can be looked
// into. The arrows and the wheel walk the timeline without leaving the viewer,
// so a project can be read straight through.

import type { Entry, Project } from "./api";
import { assetUrl } from "./api";
import { onDateFormatChange } from "./dates";
import { closeModal, openModal } from "./modal";
import { dateToggle } from "./timeline";
import { el } from "./ui";

/** How far a click into the picture magnifies it. */
const ZOOM = 2.5;

/** Wheel delta to accumulate before stepping to the next entry. */
const WHEEL_STEP = 60;

/** Pointer movement, in pixels, past which a press was a drag and not a click. */
const DRAG_SLOP = 4;

export interface ViewerContext {
  project: () => Project;
  /** Re-read the entry from the latest project state, or null if it is gone. */
  entry: (id: string) => Entry | null;
  /** The entries in the order the page shows them. */
  entries: () => Entry[];
  /**
   * The viewer is showing a different entry.
   *
   * Walking the timeline is not a move: it records which entry is on screen
   * without pushing a place, so Back leaves the viewer rather than retracing
   * every step one picture at a time.
   */
  moved: (id: string) => void;
}

interface OpenViewer {
  id: string;
  context: ViewerContext;
  stage: HTMLElement;
  caption: HTMLElement;
  /** The picture, or null for an entry that has none. */
  image: HTMLImageElement | null;
  /** What the stage was last drawn from, so a rescan only rebuilds a changed picture. */
  shown: string;
  /** Magnified, and therefore pannable. */
  zoomed: boolean;
  panX: number;
  panY: number;
}

let viewer: OpenViewer | null = null;

export function openLightbox(id: string, context: ViewerContext): void {
  if (!context.entry(id)) return;

  const stage = el("div", { class: "viewer__stage" });
  const caption = el("div", { class: "viewer__caption" });
  const body = el("div", { class: "viewer" }, stage, caption);

  bindPointer(stage);

  openModal({
    // No header: the date is in the caption, under the picture it belongs to.
    title: null,
    class: "modal--viewer",
    body,
    onClose: () => {
      viewer = null;
      wheelTowards = 0;
    },
  });

  // After `openModal`, which dismisses whatever was there and so clears this.
  viewer = {
    id,
    context,
    stage,
    caption,
    image: null,
    shown: "",
    zoomed: false,
    panX: 0,
    panY: 0,
  };
  draw({ reset: true });
}

/**
 * Bring the viewer up to date after a rescan.
 *
 * Keeps the magnification: a rescan is the app's own writes coming back, and
 * losing your place in a picture every time something on disk settles would
 * make the viewer unusable while anything else is happening.
 */
export function refreshLightbox(id: string, context: ViewerContext): void {
  // Not the entry on screen — or the viewer is not open. Either way there is
  // nothing here to correct.
  if (!viewer || viewer.id !== id) return;
  viewer.context = context;
  draw();
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
  draw({ reset: true });
}

/**
 * Redraw whatever has changed.
 *
 * `reset` is for arriving at a different entry, where carrying a magnification
 * over would land the next picture cropped to a corner of the one before.
 */
function draw(options: { reset?: boolean } = {}): void {
  if (!viewer) return;
  const { context, stage, caption } = viewer;
  const entry = context.entry(viewer.id);

  if (options.reset) {
    viewer.zoomed = false;
    viewer.panX = 0;
    viewer.panY = 0;
  }

  if (!entry) {
    viewer.image = null;
    viewer.shown = "gone";
    viewer.zoomed = false;
    stage.replaceChildren(
      el("p", { class: "empty", text: "This entry is gone." }),
    );
    caption.replaceChildren();
    applyTransform();
    return;
  }

  // Rebuilding an unchanged picture would reload it, and a reload is a flash of
  // nothing where the picture was.
  const shown = `${entry.id}::${entry.image ?? ""}`;
  if (shown !== viewer.shown) {
    viewer.shown = shown;
    viewer.image = entry.image
      ? (el("img", {
          class: "viewer__image",
          src: assetUrl(
            context.project().root,
            "entries",
            entry.id,
            entry.image,
          ),
          alt: entry.text || "Entry illustration",
        }) as HTMLImageElement)
      : null;
    stage.replaceChildren(
      viewer.image ??
        el("div", { class: "viewer__placeholder", text: "No image chosen" }),
    );
  }

  drawCaption(entry);
  applyTransform();
}

/**
 * The date and the note, each in a frame of its own.
 *
 * Two frames rather than one pill that fits its sentence: the date then starts
 * at the same place on every entry, and walking the timeline does not shuffle
 * it about under the eye.
 */
function drawCaption(entry: Entry): void {
  if (!viewer) return;
  viewer.caption.replaceChildren(
    el("div", { class: "viewer__date" }, dateToggle(entry)),
    el(
      "div",
      { class: "viewer__note" },
      el(
        "p",
        {
          class: entry.text
            ? "viewer__text"
            : "viewer__text viewer__text--empty",
        },
        entry.text || "No note yet",
      ),
    ),
  );
}

// --- zoom and pan ----------------------------------------------------------
//
// The picture is laid out by `object-fit: contain`, so the element is the whole
// stage and the picture sits letterboxed inside it. Magnifying is therefore a
// transform on the element, and the geometry below is about the picture's own
// rectangle within it — which is what panning has to be clamped against, and
// what tells a click on the picture from a click on the bare stage beside it.

/** Where the picture actually is: its size within the stage, and the stage's. */
function geometry(): {
  stage: DOMRect;
  width: number;
  height: number;
} | null {
  const image = viewer?.image;
  if (!viewer || !image || !image.naturalWidth) return null;
  const stage = viewer.stage.getBoundingClientRect();
  const fit = Math.min(
    stage.width / image.naturalWidth,
    stage.height / image.naturalHeight,
  );
  return {
    stage,
    width: image.naturalWidth * fit,
    height: image.naturalHeight * fit,
  };
}

/** Keep the pan within the range that still leaves the picture covering the stage. */
function clampPan(): void {
  if (!viewer) return;
  const geo = geometry();
  if (!geo || !viewer.zoomed) {
    viewer.panX = 0;
    viewer.panY = 0;
    return;
  }
  const limitX = Math.max(0, (geo.width * ZOOM - geo.stage.width) / 2);
  const limitY = Math.max(0, (geo.height * ZOOM - geo.stage.height) / 2);
  viewer.panX = Math.max(-limitX, Math.min(limitX, viewer.panX));
  viewer.panY = Math.max(-limitY, Math.min(limitY, viewer.panY));
}

function applyTransform(): void {
  if (!viewer) return;
  clampPan();
  const { image, stage, zoomed, panX, panY } = viewer;
  if (image) {
    image.style.transform = zoomed
      ? `translate(${panX}px, ${panY}px) scale(${ZOOM})`
      : "";
  }
  stage.classList.toggle("viewer__stage--zoomed", zoomed);
}

/** Whether a point in the window is on the picture rather than beside it. */
function onPicture(clientX: number, clientY: number): boolean {
  if (!viewer) return false;
  // No picture at all: the stage is bare from edge to edge, and a click
  // anywhere on it is a click beside nothing.
  if (!viewer.image) return false;
  const geo = geometry();
  // There is a picture, but it has not arrived yet and so has no rectangle to
  // be inside. A click while it loads must not be read as a click past it and
  // take the viewer down.
  if (!geo) return true;
  // Magnified, the picture covers the stage in at least one direction and the
  // bars are gone; treat the whole stage as picture rather than doing the
  // arithmetic twice.
  if (viewer.zoomed) return true;
  const dx = Math.abs(clientX - (geo.stage.left + geo.stage.width / 2));
  const dy = Math.abs(clientY - (geo.stage.top + geo.stage.height / 2));
  return dx <= geo.width / 2 && dy <= geo.height / 2;
}

/**
 * Clicking and dragging on the stage.
 *
 * A click on the picture magnifies it around the point clicked, and a click on
 * the magnified picture puts it back. A click on the bare stage beside a
 * picture that does not fill it dismisses, which is what a click there would do
 * over any other dialog.
 *
 * A drag moves the picture pixel for pixel with the pointer, so it stays under
 * the hand that is moving it.
 *
 * The way a drag used to get stuck was a release the stage never saw: let go
 * outside the window and no `pointerup` arrives, so the drag stayed live and
 * the picture went on following a pointer with no button held. Hence the check
 * on `buttons` and the two backstops at the bottom of this file.
 */
let dragging = false;
let lastX = 0;
let lastY = 0;
let travelled = 0;

function endDrag(): void {
  dragging = false;
}

function bindPointer(stage: HTMLElement): void {
  stage.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    dragging = true;
    travelled = 0;
    lastX = event.clientX;
    lastY = event.clientY;
    stage.setPointerCapture(event.pointerId);
  });

  stage.addEventListener("pointermove", (event) => {
    if (!dragging || !viewer) return;
    if ((event.buttons & 1) === 0) {
      endDrag();
      return;
    }
    const dx = event.clientX - lastX;
    const dy = event.clientY - lastY;
    lastX = event.clientX;
    lastY = event.clientY;
    travelled += Math.abs(dx) + Math.abs(dy);
    if (!viewer.zoomed) return;
    viewer.panX += dx;
    viewer.panY += dy;
    applyTransform();
  });

  stage.addEventListener("pointerup", (event) => {
    if (!dragging || !viewer) return;
    const wasDrag = travelled > DRAG_SLOP;
    endDrag();
    if (stage.hasPointerCapture(event.pointerId)) {
      stage.releasePointerCapture(event.pointerId);
    }
    // A drag that happened to end where it started is still a drag.
    if (wasDrag) return;

    if (viewer.zoomed) {
      viewer.zoomed = false;
      applyTransform();
      return;
    }
    if (!onPicture(event.clientX, event.clientY)) {
      closeModal();
      return;
    }
    zoomAt(event.clientX, event.clientY);
  });

  stage.addEventListener("pointercancel", endDrag);
}

// The release that ends a drag may happen where the stage never sees it —
// outside the window, or with the window losing focus underneath it.
window.addEventListener("pointerup", endDrag);
window.addEventListener("blur", endDrag);

/** Magnify about a point, so whatever was under the cursor stays under it. */
function zoomAt(clientX: number, clientY: number): void {
  const geo = geometry();
  if (!viewer || !geo) return;
  const fromCentreX = clientX - (geo.stage.left + geo.stage.width / 2);
  const fromCentreY = clientY - (geo.stage.top + geo.stage.height / 2);
  viewer.zoomed = true;
  // The transform scales about the centre, so a point `d` from it lands at
  // `ZOOM * d`; this is the translation that undoes that for the point clicked.
  viewer.panX = fromCentreX * (1 - ZOOM);
  viewer.panY = fromCentreY * (1 - ZOOM);
  applyTransform();
}

// --- walking the timeline ---------------------------------------------------

/**
 * Wheel travel since the last step, so one flick is one entry.
 *
 * A wheel reports a stream of small deltas rather than one event per notch, and
 * a trackpad reports dozens; without a threshold a single gesture would run off
 * the end of the project.
 */
let wheelTowards = 0;

window.addEventListener(
  "wheel",
  (event) => {
    if (!viewer) return;
    // The page behind is already locked, but the overlay is not a scroller and
    // the browser would otherwise look for one.
    event.preventDefault();

    // The same whether or not the picture is magnified: the wheel is how you
    // move through the project, and a gesture that changed meaning depending on
    // how closely you happened to be looking would be a trap. Dragging is how
    // you get around a magnified picture.
    const delta = event.deltaY + event.deltaX;
    // Turning back mid-gesture starts the count again rather than cancelling
    // out what has already been wound up.
    if (delta * wheelTowards < 0) wheelTowards = 0;
    wheelTowards += delta;
    if (Math.abs(wheelTowards) < WHEEL_STEP) return;
    // Down and right are further along the page, which is the direction the
    // right arrow goes.
    step(wheelTowards > 0 ? 1 : -1);
    wheelTowards = 0;
  },
  { passive: false },
);

/** Back along the page, and forward. Up and down because the page is a column. */
const ARROWS = new Map([
  ["ArrowLeft", -1],
  ["ArrowUp", -1],
  ["ArrowRight", 1],
  ["ArrowDown", 1],
]);

window.addEventListener("keydown", (event) => {
  if (!viewer || event.ctrlKey || event.metaKey || event.altKey) return;
  const delta = ARROWS.get(event.key);
  if (delta === undefined) return;
  event.preventDefault();
  step(delta);
});

// The caption's date is a toggle like any other, and flips the whole page. The
// page behind redraws itself; the caption has to follow, or the one date the
// user actually clicked is the one that does not change.
onDateFormatChange(() => {
  const entry = viewer?.context.entry(viewer.id);
  if (entry) drawCaption(entry);
});
