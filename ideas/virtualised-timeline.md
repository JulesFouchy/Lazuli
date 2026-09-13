---
summary: Window the timeline so a project with thousands of entries scrolls well
affects: [src/timeline.ts]
---

# Virtualised timeline

Every entry becomes a DOM card. At a couple of thousand entries — a realistic size for a project kept for years — scrolling will suffer.

## Why not yet

One thing makes it more than a mechanical change:

- **Scroll anchoring.** Mounting and unmounting cards while the user scrolls will make the viewport jump unless offsets are maintained deliberately.

Every gap connector has the same fixed height, so rows are uniform apart from the cards' own image heights, and the connectors need no special handling.

And plain DOM may well hold up for far longer than expected. Measure before paying that cost.

## How

Render only what is near the viewport, with a spacer above and below sized to the rows that are not mounted. Simplest workable version: measure a card once, assume a uniform height per card plus the gap height, and correct as real heights become known. Needs the cards keyed by entry id, which [incremental-timeline-render.md](incremental-timeline-render.md) also wants.

## When

When scrolling a real project actually feels bad. The cheapest way to find out is `node scripts/make-fixture.mjs <folder> 2000` and scrolling it.
