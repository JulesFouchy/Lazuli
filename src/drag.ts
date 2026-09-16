// Dragging a row to somewhere else.
//
// Built on pointer events rather than HTML5 drag-and-drop. The window already
// listens for files being dropped onto it — that is how images are added — and
// on Windows the WebView2 host takes the OS drag over, so a `dragstart` inside
// the page competes with it. Pointer events are also the only way to get a
// pointer *capture*, which is what keeps a drag alive when the pointer leaves
// the row it started on.

/** Somewhere a row can be dropped that is not a place in its own list. */
export interface DropZone {
  node: HTMLElement;
  /** Whether dropping here would do anything; a false one is not offered. */
  live?: boolean;
  onDrop: () => void;
}

export interface DragOptions {
  /** The element being dragged. It is moved within its own parent. */
  node: HTMLElement;
  /** The rows it is among, in the order they are shown, itself included. */
  items: () => HTMLElement[];
  /** Which way the list runs. A strip of tabs is the horizontal one. */
  axis?: "x" | "y";
  /** Targets outside the list, such as the tabs. */
  zones?: () => DropZone[];
  /** Dropped in the list, at this index among the rows. */
  onDrop: (index: number) => void;
  /** Called once the drag is real, which is after the threshold is passed. */
  onStart?: () => void;
  /** Called however the drag ends, including a cancel. */
  onEnd?: () => void;
}

/**
 * How far the pointer moves before this is a drag and not a click.
 *
 * Nothing happens below it, so a click on a row still opens the project and a
 * right-click still opens its menu.
 */
const THRESHOLD = 5;

/** The ghost over a drop target: how far below the pointer, and how small. */
const CHIP_OFFSET = 14;
const CHIP_SCALE = 0.4;

/** How close to the edge of the scroller the pointer pulls the list along. */
const SCROLL_MARGIN = 64;
const SCROLL_SPEED = 14;

export function beginDrag(event: PointerEvent, options: DragOptions): void {
  // The left button only: a right-click is for the context menu, and the
  // middle one for nothing here.
  if (event.button !== 0) return;

  const { node } = options;
  const parent = node.parentElement;
  if (!parent) return;

  const startX = event.clientX;
  const startY = event.clientY;
  const origin = node.nextElementSibling;
  const pointer = event.pointerId;

  let ghost: HTMLElement | null = null;
  let rect: DOMRect | null = null;
  let zone: DropZone | null = null;
  let scrolling = 0;
  let dragging = false;
  /** The last horizontal position, which an autoscroll re-places the row at. */
  let lastX = startX;

  const scroller = scrollerOf(node);

  const start = (): void => {
    rect = node.getBoundingClientRect();
    ghost = node.cloneNode(true) as HTMLElement;
    ghost.classList.add("drag-ghost");
    ghost.style.width = `${rect.width}px`;
    ghost.style.height = `${rect.height}px`;
    ghost.style.left = `${rect.left}px`;
    ghost.style.top = `${rect.top}px`;
    document.body.append(ghost);
    node.classList.add("drag-source");
    // So the drag survives the pointer leaving the row, which it does at once:
    // the row moves out from under it.
    node.setPointerCapture(pointer);
    document.body.classList.add("dragging");
    options.onStart?.();
  };

  const place = (clientX: number, clientY: number): void => {
    if (!ghost || !rect) return;

    zone =
      options.zones?.().find((candidate) => {
        if (candidate.live === false) return false;
        const area = candidate.node.getBoundingClientRect();
        return (
          clientX >= area.left &&
          clientX <= area.right &&
          clientY >= area.top &&
          clientY <= area.bottom
        );
      }) ?? null;

    for (const candidate of options.zones?.() ?? []) {
      candidate.node.classList.toggle("drop-target", candidate === zone);
    }

    // Over a tab, the row is not going anywhere in this list, so it goes back
    // to where it started rather than following the pointer to the top of it.
    //
    // The ghost shrinks to a card beside the pointer, because a row at full
    // size is wider than the whole strip and covers the very tab it is being
    // dropped on — the one thing that has to stay visible.
    if (zone) {
      ghost.style.transformOrigin = "0 0";
      ghost.style.transform = `translate(${clientX - rect.left + CHIP_OFFSET}px, ${clientY - rect.top + CHIP_OFFSET}px) scale(${CHIP_SCALE})`;
      parent.insertBefore(node, origin);
      return;
    }

    ghost.style.transformOrigin = "";
    ghost.style.transform = `translate(${clientX - startX}px, ${clientY - startY}px)`;

    // Past the middle of a neighbour is past that neighbour, measured along
    // whichever way the list runs.
    const along = options.axis === "x" ? clientX : clientY;
    const others = options.items().filter((item) => item !== node);
    const before = others.find((item) => {
      const area = item.getBoundingClientRect();
      return options.axis === "x"
        ? along < area.left + area.width / 2
        : along < area.top + area.height / 2;
    });
    parent.insertBefore(node, before ?? null);
  };

  const autoScroll = (clientY: number): void => {
    cancelAnimationFrame(scrolling);
    // Only a list that runs the same way the page scrolls. Dragging a tab
    // along its strip has nothing to reach that is off the bottom, and the
    // strip sits near enough the top of the page to be inside the margin.
    if (!scroller || options.axis === "x") return;
    const area =
      scroller === document.scrollingElement
        ? new DOMRect(0, 0, window.innerWidth, window.innerHeight)
        : scroller.getBoundingClientRect();
    const up = clientY - area.top;
    const down = area.bottom - clientY;
    const by =
      up < SCROLL_MARGIN
        ? -SCROLL_SPEED * (1 - up / SCROLL_MARGIN)
        : down < SCROLL_MARGIN
          ? SCROLL_SPEED * (1 - down / SCROLL_MARGIN)
          : 0;
    if (by === 0) return;
    const step = (): void => {
      scroller.scrollTop += by;
      // The row is placed again as the list slides under the pointer, which
      // has not moved: without this the gap stays where it was on screen.
      place(lastX, clientY);
      scrolling = requestAnimationFrame(step);
    };
    scrolling = requestAnimationFrame(step);
  };

  const onMove = (move: PointerEvent): void => {
    if (move.pointerId !== pointer) return;
    if (!dragging) {
      if (
        Math.abs(move.clientX - startX) < THRESHOLD &&
        Math.abs(move.clientY - startY) < THRESHOLD
      ) {
        return;
      }
      // Repainted out of the page between the press and the move — a tab
      // name committed by the same press, say. There is nothing left to drag,
      // and a capture on a detached node throws.
      if (!node.isConnected) {
        finish(false);
        return;
      }
      dragging = true;
      start();
    }
    lastX = move.clientX;
    place(move.clientX, move.clientY);
    autoScroll(move.clientY);
  };

  const finish = (dropped: boolean): void => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onCancel);
    window.removeEventListener("keydown", onKey, true);
    cancelAnimationFrame(scrolling);
    if (!dragging) return;

    ghost?.remove();
    node.classList.remove("drag-source");
    document.body.classList.remove("dragging");
    for (const candidate of options.zones?.() ?? []) {
      candidate.node.classList.remove("drop-target");
    }
    if (node.hasPointerCapture(pointer)) node.releasePointerCapture(pointer);

    // The click that a pointerup brings with it would open the project that
    // was just dragged. Dropped in place or not, the gesture was a drag.
    const swallow = (click: Event): void => {
      click.stopPropagation();
      click.preventDefault();
    };
    window.addEventListener("click", swallow, true);
    setTimeout(() => window.removeEventListener("click", swallow, true));

    if (dropped) {
      if (zone) {
        zone.onDrop();
      } else {
        options.onDrop(options.items().indexOf(node));
      }
    } else {
      parent.insertBefore(node, origin);
    }
    options.onEnd?.();
  };

  const onUp = (up: PointerEvent): void => {
    if (up.pointerId === pointer) finish(true);
  };
  const onCancel = (cancelled: PointerEvent): void => {
    if (cancelled.pointerId === pointer) finish(false);
  };
  const onKey = (key: KeyboardEvent): void => {
    if (key.key !== "Escape") return;
    key.stopPropagation();
    finish(false);
  };

  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onCancel);
  window.addEventListener("keydown", onKey, true);
}

/** The nearest ancestor that scrolls, which is what a drag near its edge moves. */
function scrollerOf(node: HTMLElement): HTMLElement | null {
  for (
    let parent = node.parentElement;
    parent;
    parent = parent.parentElement
  ) {
    const { overflowY } = getComputedStyle(parent);
    if (overflowY === "auto" || overflowY === "scroll") return parent;
  }
  return document.scrollingElement as HTMLElement | null;
}
