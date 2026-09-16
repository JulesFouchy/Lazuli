# Ideas

Things decided on, but deliberately not built yet. One file per idea.

The directory listing is the index — there is no summary file to fall out of date.

**Name a file after whatever will make someone open it**, in kebab-case, which is not always the idea itself. A note saying "this constant should become a setting one day" belongs in `user-settings.md`, not `day-start-hour-should-be-configurable.md`: the day it matters is the day someone builds settings, and that is the name they will be looking for. Several small deferrals often collapse into one such file.

Each file opens with frontmatter:

```yaml
---
summary: one line, so a listing of the folder is readable
affects: [src/timeline.ts]   # files this would touch, or omit if none exist yet
---
```

`affects` is the useful part: before changing a file, `rg "src/timeline" ideas/` says what is already planned for it.

Then say what the problem is, why it is not being done yet, and roughly how. The "why not yet" matters most — without it the idea gets either forgotten or picked up at the wrong moment.

**Delete the file when the idea ships.** The commit that implements it is the record; a folder of `status: done` files is just noise to read past.

## rejected/

Ideas considered and turned down go in `rejected/`, keeping the top level to things that might still happen.

Worth doing because `affects` works here too: `rg "src/video" ideas/` surfaces *"we thought about this and said no"* before someone cheerfully reimplements it. Each file should say what would have to change for the answer to become yes — a rejection is a decision about today, not a law.

**Keep the bar high.** Only record a rejection whose reasoning is non-obvious, or that someone would plausibly propose again. Everything else is noise, and a folder nobody trusts to be worth reading is worse than no folder.

A decision that constrains how code must be written is not a rejected idea — that belongs in a comment next to the code, where it cannot be missed. The reason `DAY_START_HOUR` must never go in `lazuli.yaml` lives in `dates.rs`, not here.
