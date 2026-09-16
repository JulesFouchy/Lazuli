// The Appearance dialog: theme and accent colour.

import { openModal } from "./modal";
import {
  ACCENT_PRESETS,
  accentColour,
  activeTheme,
  BG_PRESETS,
  backgroundColour,
  backgroundColourFor,
  customColour,
  normaliseHex,
  onAppearanceChange,
  rememberCustomColour,
  setAccent,
  setBackground,
  setThemeChoice,
  themeChoice,
  type Theme,
  type ThemeChoice,
  type Well,
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

  // What each well shows, which is the last colour mixed *in that well* rather
  // than the colour in force — see `paintSwatches`. It outlives the dialog, so
  // a well that has been mixed in once opens on that colour rather than on
  // whichever preset happens to be selected.
  let wellAccent = customColour("accent") ?? accentColour();
  const wellGround: Record<Theme, string> = {
    dark: customColour("dark") ?? backgroundColourFor("dark"),
    light: customColour("light") ?? backgroundColourFor("light"),
  };

  // Which of a row's swatches wears the ring, when the well happens to hold a
  // colour that is also on the palette. Two rings for one colour says the
  // setting is in two places; the one that was clicked is the true answer.
  //
  // On opening there is nothing to have clicked, so it is derived: a colour
  // that is not on the palette can only have come from the well.
  const fromWell: Record<Well, boolean> = {
    accent: !onPalette(ACCENT_PRESETS, accentColour()),
    dark: !onPalette(BG_PRESETS.dark, backgroundColourFor("dark")),
    light: !onPalette(BG_PRESETS.light, backgroundColourFor("light")),
  };

  const customGround = el("input", {
    class: "swatch swatch--custom",
    type: "color",
    title: "The colour you mixed. Click to use it, and to change it",
    "aria-label": "Custom background colour",
    // Not `preventDefault`: the OS picker opens on this same click, which is
    // the other half of what it is for.
    onclick: () => {
      fromWell[activeTheme()] = true;
      setBackground(wellGround[activeTheme()]);
    },
    oninput: (event: Event) => {
      const hex = (event.target as HTMLInputElement).value;
      wellGround[activeTheme()] = hex;
      fromWell[activeTheme()] = true;
      rememberCustomColour(activeTheme(), hex);
      setBackground(hex);
    },
  });

  // The native colour well. `input` rather than `change`, so dragging around
  // the picker repaints the app as it goes.
  const custom = el("input", {
    class: "swatch swatch--custom",
    type: "color",
    title: "The colour you mixed. Click to use it, and to change it",
    "aria-label": "Custom accent colour",
    onclick: () => {
      fromWell.accent = true;
      setAccent(wellAccent);
    },
    oninput: (event: Event) => {
      const hex = (event.target as HTMLInputElement).value;
      wellAccent = hex;
      fromWell.accent = true;
      rememberCustomColour("accent", hex);
      setAccent(hex);
    },
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

    paintSwatches({
      row: swatchRow,
      well: custom,
      wellHex: wellAccent,
      wellChosen: fromWell.accent,
      presets: ACCENT_PRESETS,
      current: accentColour(),
      pick: (hex) => {
        fromWell.accent = false;
        setAccent(hex);
      },
    });
    // The grounds on offer are the ones that belong to the theme on screen —
    // a dark one is no use while the page is paper.
    paintSwatches({
      row: groundRow,
      well: customGround,
      wellHex: wellGround[activeTheme()],
      wellChosen: fromWell[activeTheme()],
      presets: BG_PRESETS[activeTheme()],
      current: backgroundColour(),
      pick: (hex) => {
        fromWell[activeTheme()] = false;
        setBackground(hex);
      },
    });
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
 *
 * `wellHex` is the well's own colour and is not `current`. Stepping over to a
 * preset to compare it against what you mixed must not be what destroys what
 * you mixed — the well holds it until it is next used, so the comparison runs
 * both ways.
 *
 * `wellChosen` breaks the tie when the well holds a colour that is on the
 * palette too, so that the row has exactly one ring on it rather than two.
 */
function paintSwatches(options: {
  row: HTMLElement;
  well: HTMLInputElement;
  wellHex: string;
  wellChosen: boolean;
  presets: { name: string; hex: string }[];
  current: string;
  pick: (hex: string) => void;
}): void {
  const { row, well, wellHex, wellChosen, presets, current, pick } = options;

  const ringed = (hex: string): boolean => current === normaliseHex(hex);
  const wellRinged = wellChosen && ringed(wellHex);

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
        "aria-pressed": String(ringed(hex) && !wellRinged),
        onclick: () => pick(hex),
      }),
    ),
  );
  well.value = wellHex;
  well.dataset.chosen = String(wellRinged);
}

/** Whether `hex` is one of the offered colours. */
const onPalette = (
  presets: { name: string; hex: string }[],
  hex: string,
): boolean => presets.some((preset) => normaliseHex(preset.hex) === hex);
