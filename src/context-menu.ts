// A single-instance right-click menu.
//
// Built in the page rather than as a native OS menu: it needs no capability,
// it styles like the rest of the app, and it can be driven in a test the same
// way a user drives it.

import { el } from "./ui";

export interface MenuItem {
  label: string;
  run: () => void;
  /** Styled as a destructive action. */
  danger?: boolean;
}

let current: HTMLElement | null = null;

/** Distance kept from the window edge when a menu would overflow it. */
const EDGE_MARGIN = 8;

export function openContextMenu(event: MouseEvent, items: MenuItem[]): void {
  event.preventDefault();
  closeContextMenu();
  if (items.length === 0) return;

  const menu = el(
    "div",
    { class: "menu", role: "menu" },
    ...items.map((item) =>
      el("button", {
        class: item.danger ? "menu__item menu__item--danger" : "menu__item",
        role: "menuitem",
        text: item.label,
        onclick: () => {
          // Close first: the action may open a dialog, and a menu left hanging
          // over it would be the top thing on screen.
          closeContextMenu();
          item.run();
        },
      }),
    ),
  );

  // Off-screen first, so it can be measured before it is seen.
  menu.style.left = "-9999px";
  menu.style.top = "-9999px";
  document.body.append(menu);
  const { width, height } = menu.getBoundingClientRect();
  const limit = (wanted: number, size: number, available: number) =>
    Math.max(EDGE_MARGIN, Math.min(wanted, available - size - EDGE_MARGIN));
  menu.style.left = `${limit(event.clientX, width, window.innerWidth)}px`;
  menu.style.top = `${limit(event.clientY, height, window.innerHeight)}px`;

  current = menu;
}

export function closeContextMenu(): void {
  current?.remove();
  current = null;
}

export const isContextMenuOpen = () => current !== null;

// Anything that moves the page out from under the menu dismisses it. Listening
// on the capture phase so a click lands on the menu item but nowhere else.
window.addEventListener(
  "pointerdown",
  (event) => {
    if (current && !current.contains(event.target as Node)) closeContextMenu();
  },
  true,
);
window.addEventListener("blur", closeContextMenu);
window.addEventListener("resize", closeContextMenu);
window.addEventListener("scroll", closeContextMenu, true);
