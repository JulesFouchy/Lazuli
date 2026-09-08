---
summary: A settings UI, and the values that should move into it
affects: [src-tauri/src/dates.rs, src-tauri/src/commands.rs, src/dates.ts, src/main.ts]
---

# User settings

There is already a settings file — `settings.json` in the app config dir, read
and written by `commands.rs` — but it holds exactly one value, is written only
as a side effect of creating a project, and has no UI. Several things that
should be preferences are scattered elsewhere as a result.

**This file exists to be found on the day someone builds a settings screen.**
It lists what should end up in it.

## What should become a setting

### `DAY_START_HOUR` — the real one

`dates.rs` hardcodes `5`: a journal day runs 05:00 to 04:59, so an entry
written at 01:00 belongs to the previous day. Five is a guess at when a night
owl stops working, and reasonable people differ — someone who habitually works
until 06:00 wants a later hour.

Two constraints when moving it:

- **It is global, not per-project.** It describes when *you* sleep, not
  anything about a project. It must not go in `journaley.yaml`, or the same
  entry would land on different days in different projects.
- **`journal_date_at` already takes the hour as a parameter**, precisely so
  this is a one-line change at the call site rather than a refactor. Do not
  thread it through as an argument everywhere; give `dates.rs` a way to read
  the setting once.

Changing it silently rewrites history: every entry near the old boundary moves
to a different journal day, which shifts day numbers and gap lengths. The UI
should say so before applying it.

### `dateFormat` and `newestFirst` — currently in `localStorage`

`dates.ts` and `main.ts` keep these in the webview's `localStorage`. That works,
but they are lost if the WebView2 profile is cleared, and they are invisible to
anything outside the webview. They belong in `settings.json` with everything
else. Low value on their own — worth doing only as part of the same pass.

### `projects_dir` — already there

Where the "new project" dialog opens. Currently only ever set implicitly, by
creating a project somewhere. A settings screen should let it be chosen
directly.

## Why not yet

One hardcoded constant does not justify a settings screen, and 05:00 has not
yet been wrong for anyone. Build this when there are two or three things worth
putting in it, which this file will tell you.
