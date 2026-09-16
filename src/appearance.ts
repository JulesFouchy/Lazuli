// The Appearance dialog: theme and accent colour.

import { openModal } from "./modal";
import {
  ACCENT_PRESETS,
  accentColour,
  activeTheme,
  BG_PRESETS,
  backgroundColour,
  normaliseHex,
  onAppearanceChange,
  setAccent,
  setBackground,
  setThemeChoice,
  themeChoice,
  type ThemeChoice,
} from "./theme";
import { el } from "./ui";

const THEMES: { value: ThemeChoice; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/**
 * There is no Save and no Cancel: every control applies as it is touched, and
 * the page behind the dialog is the preview. A setting you can see the effect
 * of does not need to be confirmed, and an accent chosen against a swatch
 * rather than against the app is chosen blind.
 */
export function openAppearanceDialog(): void {
  const body = el("div", { class: "modal__body-inner" });

  const themeRow = el("div", { class: "segmented" });

  const customGround = el("input", {
    class: "swatch swatch--custom",
    type: "color",
    title: "Any other colour",
    "aria-label": "Custom background colour",
    oninput: (event: Event) =>
      setBackground((event.target as HTMLInputElement).value),
  });

  // The native colour well. `input` rather than `change`, so dragging around
  // the picker repaints the app as it goes.
  const custom = el("input", {
    class: "swatch swatch--custom",
    type: "color",
    title: "Any other colour",
    "aria-label": "Custom accent colour",
    oninput: (event: Event) =>
      setAccent((event.target as HTMLInputElement).value),
  });

  // The wells are built into their rows once and never taken out again; see
  // `paintSwatches`.
  const swatchRow = el("div", { class: "swatches" }, custom);
  const groundRow = el("div", { class: "swatches" }, customGround);

  const paint = () => {
    const choice = themeChoice();
    themeRow.replaceChildren(
      ...THEMES.map(({ value, label }) =>
        el("button", {
          class: "segmented__option",
          text: label,
          "aria-pressed": String(choice === value),
          onclick: () => setThemeChoice(value),
        }),
      ),
    );

    paintSwatches(swatchRow, custom, ACCENT_PRESETS, accentColour(), setAccent);
    // The grounds on offer are the ones that belong to the theme on screen —
    // a dark one is no use while the page is paper.
    paintSwatches(
      groundRow,
      customGround,
      BG_PRESETS[activeTheme()],
      backgroundColour(),
      setBackground,
    );
  };

  paint();
  const stop = onAppearanceChange(paint);

  body.append(
    el(
      "div",
      { class: "field" },
      el("label", { text: "Theme" }),
      themeRow,
    ),
    el(
      "div",
      { class: "field" },
      el("label", { text: "Background" }),
      groundRow,
    ),
    el(
      "div",
      { class: "field" },
      el("label", { text: "Accent" }),
      swatchRow,
      el(
        "div",
        { class: "appearance__preview" },
        el("span", { class: "appearance__sample-date", text: "Day 12" }),
        el("button", { class: "button button--primary", text: "New entry" }),
        el("button", { class: "button", text: "Appearance…" }),
      ),
    ),
  );

  openModal({ title: "Appearance", body, onClose: stop });
}

/**
 * Redraw a row of preset swatches, leaving the colour well beside them alone.
 *
 * The well must never be detached, not even for the instant `replaceChildren`
 * takes to hand the same node straight back: a detached `<input type="color">`
 * loses the OS picker open on it. Every touch of that picker repaints the app
 * and so ran this, which is why the picker shut the moment it was dragged.
 */
function paintSwatches(
  row: HTMLElement,
  well: HTMLInputElement,
  presets: { name: string; hex: string }[],
  current: string,
  pick: (hex: string) => void,
): void {
  for (const child of [...row.children]) {
    if (child !== well) child.remove();
  }
  row.prepend(
    ...presets.map(({ name, hex }) =>
      el("button", {
        class: "swatch",
        style: `background: ${hex}`,
        title: name,
        "aria-label": name,
        "aria-pressed": String(current === normaliseHex(hex)),
        onclick: () => pick(hex),
      }),
    ),
  );
  well.value = current;
}
