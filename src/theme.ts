// Light/dark themes and the customisable accent colour.
//
// Both live in `localStorage` rather than in the Rust settings file, for one
// reason: the theme has to be on the root element before the first paint, and a
// Tauri command is a round trip. `index.html` reads the same two keys in a
// blocking inline script and calls nothing here; this module is what changes
// them afterwards. The two must agree — see `bootTheme` in `index.html`.

import { getCurrentWindow } from "@tauri-apps/api/window";

import { setThemePreference } from "./api";

const THEME_KEY = "journaley.theme";
const ACCENT_KEY = "journaley.accent";

/** What the user chose. `system` follows the OS and is the default. */
export type ThemeChoice = "system" | "light" | "dark";

/** What that resolves to once the OS has been asked. */
export type Theme = "light" | "dark";

export const DEFAULT_ACCENT = "#f0a84a";

/** The offered accents. The first is the default, and the app's own colour. */
export const ACCENT_PRESETS: { name: string; hex: string }[] = [
  { name: "Amber", hex: DEFAULT_ACCENT },
  { name: "Coral", hex: "#f2705d" },
  { name: "Rose", hex: "#e8618f" },
  { name: "Violet", hex: "#9b7bf0" },
  { name: "Blue", hex: "#4d9df0" },
  { name: "Teal", hex: "#31b8a6" },
  { name: "Lime", hex: "#8bc44a" },
];

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

let choice = readChoice();
let accent = readAccent();

const listeners = new Set<() => void>();

/** Told whenever the theme or the accent changes, however it changed. */
export function onAppearanceChange(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const themeChoice = (): ThemeChoice => choice;
export const accentColour = (): string => accent;

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
  localStorage.setItem(ACCENT_KEY, normalised);
  apply();
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
  applyWindowTheme();
  for (const [name, value] of Object.entries(accentVariables(accent, theme))) {
    root.style.setProperty(name, value);
  }
  for (const listener of listeners) listener();
}

/**
 * Put the theme on the window frame as well as on the page.
 *
 * The title bar is drawn by the OS, so a light page under a dark title bar is
 * what you get unless the window is told. `null` hands the decision back to the
 * OS, which is exactly what `system` means — and is why this passes the choice
 * rather than the resolved theme.
 *
 * Failures are swallowed: the page is themed either way, and a window that will
 * not take a theme is not worth a toast.
 */
function applyWindowTheme(): void {
  void getCurrentWindow()
    .setTheme(choice === "system" ? null : choice)
    .catch(() => {});
  // The same choice, written where the next launch can find it before the
  // window is built. Nothing on screen waits for it.
  void setThemePreference(choice).catch(() => {});
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
  const paper = 0.9; // Roughly the light theme's `--bg`.
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
    : "system";
}

function readAccent(): string {
  return (
    normaliseHex(localStorage.getItem(ACCENT_KEY) ?? "") ?? DEFAULT_ACCENT
  );
}
