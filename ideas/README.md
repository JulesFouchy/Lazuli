# Ideas

Things decided on, but deliberately not built yet. One file per idea.

The directory listing is the index — there is no summary file to fall out of
date. Name a file after its idea, in kebab-case.

Each file opens with frontmatter:

```yaml
---
summary: one line, so a listing of the folder is readable
affects: [src/timeline.ts]   # files this would touch, or omit if none exist yet
---
```

`affects` is the useful part: before changing a file, `rg "src/timeline" ideas/`
says what is already planned for it.

Then say what the problem is, why it is not being done yet, and roughly how.
The "why not yet" matters most — without it the idea gets either forgotten or
picked up at the wrong moment.

**Delete the file when the idea ships or is dropped.** The commit that
implements it is the record; a folder of `status: done` files is just noise to
read past.
