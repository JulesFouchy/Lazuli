---
summary: Window the timeline so a project with thousands of entries scrolls well
affects: [src/timeline.ts]
---

# Virtualised timeline

Every entry becomes a DOM card. At a couple of thousand entries — a realistic
size for a project kept for years — scrolling will suffer.

## Why not yet

Two things make it more than a mechanical change:

- **Gap connectors are positional.** The dashed rule between two entries has a
  height derived from the days between them, so a windowing implementation
  cannot treat rows as uniform, and it has to keep the connectors consistent
  with whichever cards are currently mounted.
- **Scroll anchoring.** Mounting and unmounting cards while the user scrolls
  will make the viewport jump unless offsets are maintained deliberately.

And plain DOM may well hold up for far longer than expected. Measure before
paying that cost.

## How

Render only what is near the viewport, with a spacer above and below sized to
the rows that are not mounted. Simplest workable version: measure a card once,
assume a uniform height per card plus the known gap heights, and correct as
real heights become known.

## When

When scrolling a real project actually feels bad. The cheapest way to find out
is `node scripts/make-fixture.mjs <folder> 2000` and scrolling it.
