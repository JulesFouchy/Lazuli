---
summary: Bold and italic in entry text, and the constraint that shapes it
affects: [src/entry-editor.ts, src/timeline.ts, src/video.ts]
---

# Formatting in entry text

Entry text renders as plain text. `entry.md` keeps its `.md` extension and frontmatter anyway, so the door is open at no cost.

## The constraint, which is not obvious

**Inline only** — bold, italic, code, strikethrough. Not headings, lists, or blockquotes.

The reason is the video, not the timeline. Timeline cards are HTML and could render anything. Video frames are drawn on a `<canvas>`, which has no layout engine: block elements would mean hand-implementing list bullets, indentation and margin collapsing inside the frame renderer. Inline runs are tractable because each is just a `ctx.font` variant measured with `measureText`.

Rendering formatting on the timeline and silently dropping it from exports would be worse than not offering it at all — the export is meant to be what you saw.

## How

One parser producing a flat run list — `{ text, bold, italic, code }` — consumed by *both* the timeline and the canvas. Sharing the parse is what stops the two drifting apart. `marked`'s inline lexer gives the tokens directly; the escaping rules are not worth reimplementing.

The editor should stay a plain textarea over the Markdown source rather than becoming WYSIWYG: the file on disk is Markdown, and hiding that would make hand-editing `entry.md` in your own editor feel like a different app.

## Why not yet

Asked for and then dropped as not needed for a while. A sentence a day rarely wants emphasis.
