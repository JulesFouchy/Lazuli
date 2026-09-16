---
summary: Patch the cards a rescan changed instead of rebuilding every card
affects: [src/main.ts, src/timeline.ts]
---

# Incremental timeline render

Every filesystem event that changes anything ships the whole project over IPC and `render()` rebuilds the whole page: banner, toolbar, every card, every `<img>`. That includes every debounced save while a note is being typed, since each save changes the entry's text.

## Why not yet

At 81 entries a full rebuild is a few milliseconds and nobody sees it. At thousands it is a hitch every 400 ms while typing, and scroll position and image decode state go with it.

## How

Key cards by entry id. On a `project-changed` event, diff the new entry list against the one on screen: patch text and date in place on cards whose entry changed, insert or remove cards and the connectors either side of them, and leave the rest alone. The banner and toolbar are cheap and can keep being rebuilt.

Cheap, but not free of consequences: the banner holds the project name field, and rebuilding it under the caret is why `render` carries that field's contents and caret across by hand. Patching the banner in place instead would make `captureNameEdit` unnecessary — rebuilding it and forgetting to keep them would make the name uneditable again.

## When

When typing a note in a large project visibly stutters, or when the virtualised timeline is built, which needs the same keyed cards.
