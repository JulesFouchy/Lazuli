---
summary: A settings UI, and the values that should move into it
affects: [src-tauri/src/dates.rs, src-tauri/src/commands.rs, src/main.ts, src/appearance.ts, src/theme.ts]
---

# User settings

There is already a settings file — `settings.json` in the app config dir, read and written by `commands.rs` — but it holds exactly one value, is written only as a side effect of creating a project, and has no UI. Several things that should be preferences are scattered elsewhere as a result.

There is now one settings surface, `appearance.ts`, holding theme and accent only. It is a dialog, not a settings screen, and it deliberately reads neither `settings.json` nor anything in Rust: the theme has to be on the root element before the first paint, and a Tauri command is a round trip. Anything below that is *not* needed before the first paint should go to `settings.json` instead, whether or not it ends up in the same dialog.

**This file exists to be found on the day someone builds a settings screen.** It lists what should end up in it.

## What should become a setting

### `DAY_START_HOUR` — the real one

`dates.rs` hardcodes `5`: a journal day runs 05:00 to 04:59, so an entry written at 01:00 belongs to the previous day. Five is a guess at when a night owl stops working, and reasonable people differ — someone who habitually works until 06:00 wants a later hour.

Two constraints when moving it:

- **It is global, not per-project.** It describes when *you* sleep, not anything about a project. It must not go in `lapis.yaml`, or the same entry would land on different days in different projects.
- **`journal_date_at` already takes the hour as a parameter**, precisely so this is a one-line change at the call site rather than a refactor. Do not thread it through as an argument everywhere; give `dates.rs` a way to read the setting once.

Changing it silently rewrites history: every entry near the old boundary moves to a different journal day, which shifts day numbers and gap lengths. The UI should say so before applying it.

### `newestFirst` — currently in `localStorage`

`main.ts` keeps it in the webview's `localStorage`. That works, but it is lost if the WebView2 profile is cleared, and it is invisible to anything outside the webview. It belongs in `settings.json` with everything else. Low value on its own — worth doing only as part of the same pass.

The date format was here too, and went the other way: it is a property of the project, so it lives in `lapis.yaml`. Ask of anything else on this list whether the same is true of it before moving it here.

`theme.ts` keeps the theme and the accent in `localStorage` too, and those two should *stay* there: `index.html` reads them in a blocking inline script so the first paint is already in the right theme, which a Rust round trip cannot do. They are listed here only so a later pass does not sweep them up.

### `projects_dir` — already there

Where the "new project" dialog opens. Currently only ever set implicitly, by creating a project somewhere. A settings screen should let it be chosen directly.

## Why not yet

One hardcoded constant does not justify a settings screen, and 05:00 has not yet been wrong for anyone. The Appearance dialog is not that screen: it holds the two values that have to be read before the page paints, and nothing else. Build the real one when there are two or three things worth putting in it, which this file will tell you.
