// Date presentation, and the open project's format toggle.
//
// This mirrors the *formatting* half of `src-tauri/src/dates.rs`. It
// deliberately does not reimplement the 5am rule: `journal_date` and
// `day_number` arrive already computed on every entry, so there is exactly one
// implementation of the rule that matters.

import { setDateFormat, type DateFormat } from "./api";
import { toastError } from "./ui";

const MONTHS = [
  "Jan", "Feb", "Mar", "Apr", "May", "Jun",
  "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/**
 * The format of whichever project is open.
 *
 * A copy of what `lapis.yaml` says, so the renderers do not each have to be
 * handed the project. [`adoptDateFormat`] is what keeps it a copy.
 */
let current: DateFormat = "real";

const listeners = new Set<() => void>();

/**
 * Take the format from a project that has just arrived from Rust.
 *
 * Deliberately silent: every caller redraws immediately afterwards, and firing
 * the listeners here would make the toggle's own rescan redraw a second time.
 */
export function adoptDateFormat(format: DateFormat): void {
  current = format;
}

/**
 * Flip every date on the page at once.
 *
 * Clicking any single date toggles all of them: the format is a way of reading
 * the whole project, not a property of one entry — which is why it is stored in
 * `lapis.yaml` rather than against this machine.
 */
export function toggleDateFormat(): void {
  const previous = current;
  current = current === "real" ? "day" : "real";
  for (const listener of listeners) listener();
  // Shown first and written afterwards: a click should not wait for a file
  // write. The rescan the write triggers says the same thing, so nothing moves
  // when it lands — unless the write failed, and then the page goes back to
  // what is actually on disk.
  void setDateFormat(current).catch((err) => {
    current = previous;
    for (const listener of listeners) listener();
    toastError("Could not save the date format", err);
  });
}

export function onDateFormatChange(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** `Sep 05, 2026` — zero-padded, so the width never changes. */
export function formatRealWorld(journalDate: string): string {
  const [year, month, day] = journalDate.split("-");
  return `${MONTHS[Number(month) - 1]} ${day}, ${year}`;
}

/** `Day 39`. Zero and negative numbers are shown honestly. */
export function formatDayNumber(dayNumber: number): string {
  return `Day ${dayNumber}`;
}

/** Whichever format is currently selected. */
export function formatDate(journalDate: string, dayNumber: number): string {
  return current === "real"
    ? formatRealWorld(journalDate)
    : formatDayNumber(dayNumber);
}

/** The other format, for a tooltip so the toggle is discoverable. */
export function formatDateAlternate(
  journalDate: string,
  dayNumber: number,
): string {
  return current === "real"
    ? formatDayNumber(dayNumber)
    : formatRealWorld(journalDate);
}

/** Whole days between two `YYYY-MM-DD` journal dates. */
export function daysBetween(from: string, to: string): number {
  const ms = Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`);
  return Math.round(ms / 86_400_000);
}

/** `3 days`, `1 day`, `2 months` — the label on a timeline gap connector. */
export function formatGap(days: number): string {
  if (days < 60) return days === 1 ? "1 day" : `${days} days`;
  const months = Math.round(days / 30.44);
  if (months < 24) return `${months} months`;
  return `${(days / 365.25).toFixed(1)} years`;
}

