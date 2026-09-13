// A single-instance modal host.

import { el } from "./ui";

let current: { overlay: HTMLElement; onClose?: () => void } | null = null;

export interface ModalOptions {
  /**
   * Plain text, a node when the title is itself a control, or null for a
   * dialog with no header at all -- one whose contents say what it is, and
   * which Escape, Back and the backdrop are enough to close.
   */
  title: string | HTMLElement | null;
  body: HTMLElement;
  foot?: HTMLElement;
  onClose?: () => void;
}

/**
 * Notified when the modal layer becomes empty.
 *
 * Distinct from a modal's own `onClose`: one modal replacing another is not a
 * dismissal, and the app's history must not record it as a move.
 */
let onDismissed: (() => void) | null = null;

export function onModalDismissed(listener: () => void): void {
  onDismissed = listener;
}

export function openModal(options: ModalOptions): void {
  closeModal({ replacing: true });

  // A click's target is the common ancestor of where it went down and where it
  // came up, so dragging a text selection out of the dialog and releasing on
  // the backdrop reports the backdrop. Requiring the press to have started
  // there too is what tells a dismissal from a selection that overshot.
  let pressedBackdrop = false;

  const overlay = el(
    "div",
    {
      class: "overlay",
      onpointerdown: (event: Event) => {
        pressedBackdrop = event.target === overlay;
      },
      onclick: (event: Event) => {
        if (event.target === overlay && pressedBackdrop) closeModal();
      },
    },
    el(
      "div",
      { class: "modal", role: "dialog", "aria-modal": "true" },
      // A header is a title and nothing else. There is no Close button on any
      // dialog: Escape, the mouse's Back button and a click on the backdrop
      // all dismiss, and a button repeating that only took up the corner.
      options.title === null
        ? null
        : el(
            "header",
            { class: "modal__head" },
            typeof options.title === "string"
              ? el("h2", { class: "modal__title", text: options.title })
              : options.title,
          ),
      el("div", { class: "modal__body" }, options.body),
      options.foot ?? null,
    ),
  );

  document.body.append(overlay);
  current = { overlay, onClose: options.onClose };
}

export function closeModal(options: { replacing?: boolean } = {}): void {
  if (!current) return;
  const { overlay, onClose } = current;
  // Clear first: `onClose` may itself want to open another modal.
  current = null;
  overlay.remove();
  onClose?.();
  if (!options.replacing) onDismissed?.();
}

export const isModalOpen = () => current !== null;

/** Swap the open modal's contents without the dismiss-and-reopen flicker. */
export function replaceModalBody(body: HTMLElement): void {
  const host = current?.overlay.querySelector(".modal__body");
  if (host) host.replaceChildren(body);
}
