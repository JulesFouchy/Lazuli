// Light/dark themes and the customisable accent colour.
//
// Both live in `localStorage` rather than in the Rust settings file, for one
// reason: the theme has to be on the root element before the first paint, and a
// Tauri command is a round trip. `index.html` reads the same two keys in a
// blocking inline script and calls nothing here; this module is what changes
// them afterwards. The two must agree — see `bootTheme` in `index.html`.

import { getCurrentWindow } from "@tauri-apps/api/window";

import { setThemePreference } from "./api";

const THEME_KEY = "lazuli.theme";
const ACCENT_KEY = "lazuli.accent";

/** One per theme: a background chosen against paper is not a dark ground. */
const BG_KEY: Record<Theme, string> = {
  dark: "lazuli.bg.dark",
  light: "lazuli.bg.light",
};

/**
 * What the user chose. `system` follows the OS; `dark` is what they get until
 * they choose anything, because the app is a dark blue one and opening it as a
 * white page on a light machine shows the wrong app.
 */
export type ThemeChoice = "system" | "light" | "dark";

/** The theme before anyone has picked one. Mirrored in `index.html` and `theme.rs`. */
export const DEFAULT_THEME: ThemeChoice = "dark";

/** What that resolves to once the OS has been asked. */
export type Theme = "light" | "dark";

export const DEFAULT_ACCENT = "#ffc95c";

/** The offered accents. The first is the default, and the app's own colour. */
export const ACCENT_PRESETS: { name: string; hex: string }[] = [
  { name: "Gold", hex: DEFAULT_ACCENT },
  { name: "Amber", hex: "#f0a84a" },
  { name: "Coral", hex: "#f2705d" },
  { name: "Rose", hex: "#e8618f" },
  { name: "Violet", hex: "#9b7bf0" },
  { name: "Sky", hex: "#4d9df0" },
  { name: "Teal", hex: "#31b8a6" },
  { name: "Lime", hex: "#8bc44a" },
];

/**
 * The default ground for each theme. Kept in step with `--bg` in `styles.css`
 * and with the constants in `src-tauri/src/theme.rs`, which paints the window
 * with one of them before the page exists.
 */
export const DEFAULT_BG: Record<Theme, string> = {
  dark: "#0b1020",
  light: "#c6dafb",
};

/** The offered grounds, per theme. The first of each is that theme's default. */
export const BG_PRESETS: Record<Theme, { name: string; hex: string }[]> = {
  dark: [
    { name: "Lazuli", hex: DEFAULT_BG.dark },
    { name: "Ink", hex: "#0d0d10" },
    { name: "Bistre", hex: "#17120d" },
    { name: "Verdigris", hex: "#071613" },
    { name: "Porphyry", hex: "#150b14" },
  ],
  light: [
    { name: "Haze", hex: DEFAULT_BG.light },
    { name: "Mist", hex: "#eaeff8" },
    { name: "Paper", hex: "#fbfbfa" },
    { name: "Vellum", hex: "#f1f0ed" },
    { name: "Linen", hex: "#f3ece1" },
  ],
};

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

let choice = readChoice();
let accent = readAccent();
const background: Record<Theme, string> = {
  dark: readBackground("dark"),
  light: readBackground("light"),
};

const listeners = new Set<() => void>();

/** Told whenever the theme or the accent changes, however it changed. */
export function onAppearanceChange(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const themeChoice = (): ThemeChoice => choice;
export const accentColour = (): string => accent;

/** The ground of the theme currently on screen. */
export const backgroundColour = (): string => backgroundColourFor(activeTheme());

/** The ground of either theme, whether or not it is the one on screen. */
export const backgroundColourFor = (theme: Theme): string => background[theme];

/** The theme actually on screen, with `system` resolved. */
export function activeTheme(): Theme {
  if (choice === "system") return darkQuery.matches ? "dark" : "light";
  return choice;
}

export function setThemeChoice(next: ThemeChoice): void {
  choice = next;
  localStorage.setItem(THEME_KEY, next);
  apply();
}

export function setAccent(hex: string): void {
  const normalised = normaliseHex(hex);
  if (!normalised) return;
  accent = normalised;
  remember(ACCENT_KEY, normalised, DEFAULT_ACCENT);
  apply();
}

/** Set the ground of the theme currently on screen. */
export function setBackground(hex: string): void {
  const normalised = normaliseHex(hex);
  if (!normalised) return;
  const theme = activeTheme();
  background[theme] = normalised;
  remember(BG_KEY[theme], normalised, DEFAULT_BG[theme]);
  apply();
}

/**
 * Store a chosen colour, or forget it when the choice *is* the default.
 *
 * The two look identical right up until the default moves, and then the people
 * who never wanted anything other than the default are exactly the ones left
 * behind on the old one — they are pinned to it by a click that, at the time,
 * changed nothing. Forgetting it keeps "the default" a live answer instead of
 * a snapshot of what it happened to be the day the swatch was pressed.
 */
function remember(key: string, chosen: string, fallback: string): void {
  if (chosen === fallback) localStorage.removeItem(key);
  else localStorage.setItem(key, chosen);
}

/** Put the stored appearance on the document, and keep it in step with the OS. */
export function startTheme(): void {
  apply();
  // Only matters while the choice is `system`, but the listener is cheap and
  // unregistering it on every toggle is another thing to get wrong.
  darkQuery.addEventListener("change", () => {
    if (choice === "system") apply();
  });
}

function apply(): void {
  const theme = activeTheme();
  const root = document.documentElement;
  root.dataset.theme = theme;
  const ground = background[theme];
  applyWindowTheme();
  const variables = {
    ...accentVariables(accent, theme),
    ...backgroundVariables(ground, theme),
  };
  for (const [name, value] of Object.entries(variables)) {
    root.style.setProperty(name, value);
  }
  for (const listener of listeners) listener();
}

/**
 * Put the theme on the window frame as well as on the page.
 *
 * The window has no title bar of its own, but the OS still draws its border and
 * its shadow, and those follow the window's theme rather than the page's. `null`
 * hands the decision back to the OS, which is exactly what `system` means — and
 * is why this passes the choice rather than the resolved theme.
 *
 * Failures are swallowed: the page is themed either way, and a window that will
 * not take a theme is not worth a toast.
 */
function applyWindowTheme(): void {
  void getCurrentWindow()
    .setTheme(choice === "system" ? null : choice)
    .catch(() => {});
  // The same choice and the same grounds, written where the next launch can
  // find them before the window is built. Nothing on screen waits for it.
  void setThemePreference(choice, background.dark, background.light).catch(
    () => {},
  );
}

/**
 * The accent and everything derived from it.
 *
 * Derived here rather than with `color-mix` in the stylesheet so that the same
 * numbers are available to the canvas that draws the export frames, which has
 * no CSS at all.
 */
export function accentVariables(
  hex: string,
  theme: Theme,
): Record<string, string> {
  const [r, g, b] = toRgb(hex);
  return {
    "--accent": hex,
    // Hover moves the accent away from the page: lighter on a dark ground,
    // darker on a light one, so it reads as "more" either way.
    "--accent-hover": theme === "dark" ? lighten(hex, 0.16) : darken(hex, 0.14),
    // A wash for the ground behind an accented control. Weaker on light,
    // where a tint of a saturated colour goes muddy far sooner.
    "--accent-soft": `rgba(${r}, ${g}, ${b}, ${theme === "dark" ? 0.16 : 0.12})`,
    "--accent-line": `rgba(${r}, ${g}, ${b}, 0.55)`,
    /* Text sitting on top of the accent. */
    "--on-accent": readableInk(hex),
    // The accent as a text colour. A mid-tone accent on a light page fails
    // contrast as text, so on light it is darkened until it does not.
    "--accent-text": theme === "dark" ? hex : darkenUntilReadable(hex),
  };
}

/**
 * The surfaces derived from the chosen ground.
 *
 * Only the surfaces: text, lines and shadows stay with the theme in
 * `styles.css`. A ground is picked within its theme's range — a darker blue,
 * a warmer black — and within that range the theme's ink already reads. Let
 * this compute the ink as well and every override becomes a chance to produce
 * an unreadable page.
 *
 * The two themes need different arithmetic, not one formula with different
 * numbers. On a dark ground a raised card is the same material catching more
 * light, which is a *scaling* of the channels — that holds the hue exactly,
 * where mixing toward white would grey it out. A pale ground cannot be scaled
 * without clipping to white, so it mixes instead; and its hover is not a
 * lighter white but a less white one, because on paper there is nowhere
 * lighter than the card to go.
 */
export function backgroundVariables(
  hex: string,
  theme: Theme,
): Record<string, string> {
  const ramp =
    theme === "dark"
      ? {
          raised: scale(hex, 1.6),
          hover: scale(hex, 2.05),
          sunken: scale(hex, 0.62),
          inset: scale(hex, 1.35),
        }
      : {
          raised: lighten(hex, 0.95),
          hover: lighten(hex, 0.45),
          sunken: darken(hex, 0.055),
          inset: lighten(hex, 0.75),
        };
  return {
    "--bg": hex,
    "--bg-raised": ramp.raised,
    "--bg-raised-hover": ramp.hover,
    "--bg-sunken": ramp.sunken,
    "--bg-inset": ramp.inset,
  };
}

/** Every channel multiplied by the same factor, which is what holds the hue. */
function scale(hex: string, factor: number): string {
  const [r, g, b] = toRgb(hex);
  return fromRgb(r * factor, g * factor, b * factor);
}

/**
 * The accent as it should be drawn on an export frame.
 *
 * Frames are the accent over a dark scrim whatever the app's theme is, so this
 * ignores the theme and only asks whether the colour is light enough to read
 * there — which a deep blue or purple accent is not.
 */
export function accentOnDark(): string {
  const ink = 0.02; // Roughly the scrim at the top of a frame.
  let current = accent;
  for (let step = 0; step < 24; step += 1) {
    if ((luminance(current) + 0.05) / (ink + 0.05) >= 4.5) return current;
    current = lighten(current, 0.1);
  }
  return current;
}

/** Black or white, whichever reads better on `hex`. */
export function readableInk(hex: string): string {
  // The threshold is where contrast against black and against white cross.
  return luminance(hex) > 0.42 ? "#14140f" : "#ffffff";
}

/**
 * Darken a colour until it clears 4.5:1 against the light theme's page.
 *
 * Amber as text on white is the case that forces this: at its own lightness it
 * is barely more than a highlighter stripe.
 */
function darkenUntilReadable(hex: string): string {
  // The light theme's own ground, rather than a number written down beside it:
  // the ground has moved once already, and the copy did not.
  const paper = luminance(DEFAULT_BG.light);
  let current = hex;
  for (let step = 0; step < 24; step += 1) {
    if ((paper + 0.05) / (luminance(current) + 0.05) >= 4.5) return current;
    current = darken(current, 0.08);
  }
  return current;
}

/** `#rgb`, `#rrggbb` or a bare `rrggbb`, normalised to `#rrggbb`. */
export function normaliseHex(input: string): string | null {
  const text = input.trim().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/i.test(text)) {
    const [r, g, b] = text;
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  if (/^[0-9a-f]{6}$/i.test(text)) return `#${text.toLowerCase()}`;
  return null;
}

function toRgb(hex: string): [number, number, number] {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

function fromRgb(r: number, g: number, b: number): string {
  const channel = (n: number) =>
    Math.round(Math.min(255, Math.max(0, n)))
      .toString(16)
      .padStart(2, "0");
  return `#${channel(r)}${channel(g)}${channel(b)}`;
}

function lighten(hex: string, amount: number): string {
  const [r, g, b] = toRgb(hex);
  return fromRgb(
    r + (255 - r) * amount,
    g + (255 - g) * amount,
    b + (255 - b) * amount,
  );
}

function darken(hex: string, amount: number): string {
  const [r, g, b] = toRgb(hex);
  return fromRgb(r * (1 - amount), g * (1 - amount), b * (1 - amount));
}

/** Relative luminance, as WCAG defines it. */
function luminance(hex: string): number {
  const channels = toRgb(hex).map((value) => {
    const srgb = value / 255;
    return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function readChoice(): ThemeChoice {
  const stored = localStorage.getItem(THEME_KEY);
  return stored === "light" || stored === "dark" || stored === "system"
    ? stored
    : DEFAULT_THEME;
}

function readAccent(): string {
  return (
    normaliseHex(localStorage.getItem(ACCENT_KEY) ?? "") ?? DEFAULT_ACCENT
  );
}

function readBackground(theme: Theme): string {
  return (
    normaliseHex(localStorage.getItem(BG_KEY[theme]) ?? "") ?? DEFAULT_BG[theme]
  );
}
