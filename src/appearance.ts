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
  const swatchRow = el("div", { class: "swatches" });
  const groundRow = el("div", { class: "swatches" });

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

    const accent = accentColour();
    swatchRow.replaceChildren(
      ...ACCENT_PRESETS.map(({ name, hex }) =>
        el("button", {
          class: "swatch",
          style: `background: ${hex}`,
          title: name,
          "aria-label": name,
          "aria-pressed": String(accent === normaliseHex(hex)),
          onclick: () => setAccent(hex),
        }),
      ),
      custom,
    );
    // Assigned rather than rebuilt: replacing the input mid-drag would close
    // the picker the drag is happening in.
    custom.value = accent;

    // The grounds on offer are the ones that belong to the theme on screen —
    // a dark one is no use while the page is paper.
    const ground = backgroundColour();
    groundRow.replaceChildren(
      ...BG_PRESETS[activeTheme()].map(({ name, hex }) =>
        el("button", {
          class: "swatch",
          style: `background: ${hex}`,
          title: name,
          "aria-label": name,
          "aria-pressed": String(ground === normaliseHex(hex)),
          onclick: () => setBackground(hex),
        }),
      ),
      customGround,
    );
    customGround.value = ground;
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
