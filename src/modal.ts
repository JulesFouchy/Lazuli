// A single-instance modal host.

import { el } from "./ui";

let current: { overlay: HTMLElement; onClose?: () => void } | null = null;

export interface ModalOptions {
  title: string;
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
      el(
        "header",
        { class: "modal__head" },
        el("h2", { class: "modal__title", text: options.title }),
        el("span", { class: "card__grow" }),
        el("button", {
          class: "button button--ghost",
          text: "Close",
          onclick: () => closeModal(),
        }),
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

/** Retitle the open modal, e.g. when the date it is showing changes. */
export function setModalTitle(title: string): void {
  const heading = current?.overlay.querySelector(".modal__title");
  if (heading) heading.textContent = title;
}

/** Swap the open modal's contents without the dismiss-and-reopen flicker. */
export function replaceModalBody(body: HTMLElement): void {
  const host = current?.overlay.querySelector(".modal__body");
  if (host) host.replaceChildren(body);
}
