// A single-instance modal host.

import { el } from "./ui";

let current: { overlay: HTMLElement; onClose?: () => void } | null = null;

export interface ModalOptions {
  title: string;
  body: HTMLElement;
  foot?: HTMLElement;
  onClose?: () => void;
}

export function openModal(options: ModalOptions): void {
  closeModal();

  const overlay = el(
    "div",
    {
      class: "overlay",
      onclick: (event: Event) => {
        // Only a click on the backdrop itself dismisses.
        if (event.target === overlay) closeModal();
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

export function closeModal(): void {
  if (!current) return;
  const { overlay, onClose } = current;
  // Clear first: `onClose` may itself want to open another modal.
  current = null;
  overlay.remove();
  onClose?.();
}

export const isModalOpen = () => current !== null;

/** Swap the open modal's contents without the dismiss-and-reopen flicker. */
export function replaceModalBody(body: HTMLElement): void {
  const host = current?.overlay.querySelector(".modal__body");
  if (host) host.replaceChildren(body);
}
