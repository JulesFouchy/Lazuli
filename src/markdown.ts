// Markdown in entry notes and project names.
//
// Three views of one source, and one scanner behind all three:
//
// - `renderBlocks` — the note as it reads. Headings, lists and paragraphs,
//   markers gone. What a card and the viewer show.
// - `highlight` — the note as it is written. Every character of the source is
//   still there, the markers dimmed and the text they mark already styled, the
//   way a Markdown file looks in an editor. What the editing fields show.
// - `plainText` — the words alone, for an `alt`, a title, a filename.
//
// Hand-rolled rather than a CommonMark library, and `highlight` is the reason.
// A library turns source into HTML and throws the source away; this needs the
// source kept, with styling that points into it. Everything else follows from
// the scanner emitting the markers as tokens instead of consuming them: drop
// them and you have the reading view, keep them and you have the editing one.
//
// Not supported, and each one deliberately: nested lists, links, images,
// tables, blockquotes, code fences, footnotes, thematic breaks. A day's note is
// a sentence and sometimes a short list. Every one of these is a thing to
// render in three places and to explain to someone who typed it by accident.

/** A stretch of text carrying one set of emphases. */
export interface Run {
  text: string;
  bold: boolean;
  italic: boolean;
  code: boolean;
  strike: boolean;
}

type Style = Omit<Run, "text">;

/**
 * What the scanner emits.
 *
 * A marker is a delimiter — `**`, a backtick, the backslash of an escape — and
 * carries the style it turns on, so the asterisks of `**bold**` come out bold
 * themselves. That is what makes the editing view read as one phrase rather
 * than as punctuation with words in between.
 */
interface Token {
  text: string;
  style: Style;
  marker: boolean;
}

const PLAIN: Style = {
  bold: false,
  italic: false,
  code: false,
  strike: false,
};

/**
 * The emphasis delimiters, longest first so that `**` is never read as two `*`
 * and `***` is never read as `**` and a stray one.
 */
const DELIMITERS: { mark: string; adds: Partial<Style> }[] = [
  { mark: "***", adds: { bold: true, italic: true } },
  { mark: "___", adds: { bold: true, italic: true } },
  { mark: "**", adds: { bold: true } },
  { mark: "__", adds: { bold: true } },
  { mark: "~~", adds: { strike: true } },
  { mark: "*", adds: { italic: true } },
  { mark: "_", adds: { italic: true } },
];

// --- blocks ---------------------------------------------------------------

export type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; ordered: boolean; start: number; items: string[] };

/**
 * The line shapes. Each keeps its parts as separate groups, because `highlight`
 * has to put the line back together exactly as it was typed.
 *
 * A space after the marker is required throughout, and that is what keeps
 * `*italic*` at the start of a line from being a bullet.
 */
const HEADING = /^(#{1,6})([ \t]+)(.*)$/;
const BULLET = /^([ \t]*)([-*+])([ \t]+)(.*)$/;
const NUMBERED = /^([ \t]*)(\d{1,9}[.)])([ \t]+)(.*)$/;

/**
 * The source as blocks.
 *
 * A paragraph keeps the line breaks inside it rather than folding them into
 * spaces the way Markdown proper would: someone who pressed Enter in a note
 * meant it, and the card has always shown it. `white-space: pre-wrap` on the
 * paragraph is the other half of that.
 */
export function parseBlocks(source: string): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];

  const flush = (): void => {
    if (paragraph.length > 0) {
      blocks.push({ kind: "paragraph", text: paragraph.join("\n") });
    }
    paragraph = [];
  };

  for (const line of source.split(/\r?\n/)) {
    if (!line.trim()) {
      flush();
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      flush();
      blocks.push({
        kind: "heading",
        level: heading[1].length,
        text: heading[3].trim(),
      });
      continue;
    }

    const bullet = BULLET.exec(line);
    const numbered = bullet ? null : NUMBERED.exec(line);
    const item = bullet ?? numbered;
    if (item) {
      flush();
      const ordered = numbered !== null;
      const last = blocks[blocks.length - 1];
      // A run of items of the same kind is one list; switching between bullets
      // and numbers starts another, because the two are different lists.
      if (last?.kind === "list" && last.ordered === ordered) {
        last.items.push(item[4]);
      } else {
        blocks.push({
          kind: "list",
          ordered,
          start: ordered ? Number.parseInt(item[2], 10) : 1,
          items: [item[4]],
        });
      }
      continue;
    }

    paragraph.push(line);
  }
  flush();
  return blocks;
}

/** The blocks as DOM: what a card and the viewer show. */
export function renderBlocks(source: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  for (const block of parseBlocks(source)) {
    fragment.append(blockElement(block));
  }
  return fragment;
}

function blockElement(block: Block): HTMLElement {
  if (block.kind === "heading") {
    // `h3` and down, never `h1`: the project's name is the page's `h1`, and a
    // card is inside it. Levels 4, 5 and 6 all land on `h6`, which is a
    // distinction nobody writing a day's note is drawing.
    const node = document.createElement(
      `h${Math.min(6, block.level + 2)}` as "h3",
    );
    node.className = "md__heading";
    node.append(renderInline(block.text));
    return node;
  }

  if (block.kind === "list") {
    const list = document.createElement(block.ordered ? "ol" : "ul");
    list.className = "md__list";
    // A list that starts at 3 is numbered from 3, so `<ol>` is told where the
    // numbers the user typed began.
    if (list instanceof HTMLOListElement && block.start !== 1) {
      list.start = block.start;
    }
    for (const item of block.items) {
      const node = document.createElement("li");
      node.append(renderInline(item));
      list.append(node);
    }
    return list;
  }

  const node = document.createElement("p");
  node.className = "md__paragraph";
  node.append(renderInline(block.text));
  return node;
}

// --- the editing view ------------------------------------------------------

/**
 * The source with its markers still in it, dimmed, and the text they mark
 * already styled.
 *
 * Every character of `source` comes out, in order, so the result can sit under
 * a caret: this is what the editing fields show, and what makes them show
 * formatting without hiding the Markdown that produces it.
 */
export function highlight(source: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  const lines = source.split("\n");

  lines.forEach((line, index) => {
    if (index > 0) fragment.append(document.createTextNode("\n"));
    fragment.append(highlightLine(line));
  });
  return fragment;
}

function highlightLine(line: string): DocumentFragment {
  const fragment = document.createDocumentFragment();

  const heading = HEADING.exec(line);
  if (heading) {
    fragment.append(markerNode(heading[1] + heading[2]));
    const rest = document.createElement("span");
    rest.className = `md__heading-ink md__heading-ink--${heading[1].length}`;
    rest.append(highlightInline(heading[3]));
    fragment.append(rest);
    return fragment;
  }

  const bullet = BULLET.exec(line);
  const item = bullet ?? NUMBERED.exec(line);
  if (item) {
    fragment.append(markerNode(item[1] + item[2] + item[3]));
    fragment.append(highlightInline(item[4]));
    return fragment;
  }

  fragment.append(highlightInline(line));
  return fragment;
}

/** One inline stretch, markers included. */
function highlightInline(source: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  for (const token of scan(source, PLAIN)) {
    const node = styled(token.text, token.style);
    fragment.append(token.marker ? dim(node) : node);
  }
  return fragment;
}

function markerNode(text: string): HTMLElement {
  const node = document.createElement("span");
  node.className = "md__mark";
  node.textContent = text;
  return node;
}

function dim(inner: Node): HTMLElement {
  const node = document.createElement("span");
  node.className = "md__mark";
  node.append(inner);
  return node;
}

// --- the reading view of one inline stretch --------------------------------

/** A single inline stretch as runs: no blocks, markers dropped. */
export function parseInline(source: string): Run[] {
  return merge(
    scan(source, PLAIN)
      .filter((token) => !token.marker)
      .map((token) => ({ text: token.text, ...token.style })),
  );
}

/** A single inline stretch as DOM, markers dropped. */
export function renderInline(source: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  for (const run of parseInline(source)) {
    fragment.append(styled(run.text, run));
  }
  return fragment;
}

/**
 * Text wrapped in the tags its emphases call for.
 *
 * `code` goes innermost, so emphasis around a code span reads as emphasis on
 * the span rather than as a bolder monospace.
 */
function styled(text: string, style: Style): Node {
  const wrappers = [
    [style.code, "code"],
    [style.italic, "em"],
    [style.strike, "s"],
    [style.bold, "strong"],
  ] as const;

  let node: Node = document.createTextNode(text);
  for (const [wanted, tag] of wrappers) {
    if (!wanted) continue;
    const element = document.createElement(tag);
    element.append(node);
    node = element;
  }
  return node;
}

// --- plain text -----------------------------------------------------------

/**
 * The words alone: for an `alt`, a title, a toast, a filename.
 *
 * Blocks are joined with a single space rather than with their line breaks,
 * because every caller of this wants one line of text.
 */
export function plainText(source: string): string {
  return parseBlocks(source)
    .flatMap((block) =>
      block.kind === "list" ? block.items : [block.text],
    )
    // A paragraph's own line breaks go too: one line means one line.
    .map((text) => plainInline(text).replace(/\s+/g, " "))
    .join(" ")
    .trim();
}

const plainInline = (source: string): string =>
  scan(source, PLAIN)
    .filter((token) => !token.marker)
    .map((token) => token.text)
    .join("");

/**
 * Whether the source and what it renders to are different text.
 *
 * Which is the question "is there any markup here at all", and is asked only of
 * a project name — one line, no blocks — where a name with none can be shown
 * and edited as the same string.
 */
export function hasMarkup(source: string): boolean {
  return plainInline(source) !== source;
}

// --- the scanner ----------------------------------------------------------

/** Walk `source`, emitting tokens; recurses once per nested emphasis. */
function scan(source: string, style: Style): Token[] {
  const tokens: Token[] = [];
  let literal = "";
  const flush = (): void => {
    if (literal) tokens.push({ text: literal, style, marker: false });
    literal = "";
  };
  const mark = (text: string, inner: Style): void => {
    tokens.push({ text, style: inner, marker: true });
  };

  let at = 0;
  while (at < source.length) {
    const char = source[at];

    // A backslash before punctuation is that punctuation: the way to write
    // about asterisks in a note. The backslash is the marker and the character
    // it protects is text.
    if (char === "\\" && isPunctuation(source[at + 1] ?? "")) {
      flush();
      mark("\\", style);
      tokens.push({ text: source[at + 1], style, marker: false });
      at += 2;
      continue;
    }

    if (char === "`") {
      const span = codeSpan(source, at);
      if (span) {
        flush();
        const inner = { ...style, code: true };
        // Nothing inside a code span is a delimiter, so it is taken whole and
        // not scanned. Backslashes in it are backslashes.
        mark(span.fence, inner);
        tokens.push({ text: span.text, style: inner, marker: false });
        mark(span.fence, inner);
        at = span.end;
        continue;
      }
    }

    const emphasis = openDelimiter(source, at);
    if (emphasis) {
      flush();
      const inner = { ...style, ...emphasis.adds };
      mark(emphasis.mark, inner);
      tokens.push(...scan(emphasis.inner, inner));
      mark(emphasis.mark, inner);
      at = emphasis.end;
      continue;
    }

    literal += char;
    at += 1;
  }
  flush();
  return tokens;
}

/** The emphasis starting at `at`, if one both opens and closes there. */
function openDelimiter(
  source: string,
  at: number,
): { mark: string; inner: string; adds: Partial<Style>; end: number } | null {
  for (const { mark, adds } of DELIMITERS) {
    if (!source.startsWith(mark, at)) continue;
    if (!canOpen(source, at, mark)) continue;
    const close = findClose(source, at + mark.length, mark);
    // Nothing between the two is not an emphasis, it is a pair of literal
    // marks: without this, the leading `*` of an unclosed `**bold` closes on
    // the second one and eats them both.
    if (close <= at + mark.length) continue;
    return {
      mark,
      inner: source.slice(at + mark.length, close),
      adds,
      end: close + mark.length,
    };
  }
  return null;
}

/**
 * Whether a delimiter at `at` can open an emphasis.
 *
 * The text after it must not be a space, which is what keeps `2 * 3` as
 * arithmetic. An underscore additionally may not follow a letter or a digit,
 * which is what keeps `entry_editor.ts` and `MAX_LINES` as themselves — the one
 * rule of CommonMark's that a journal really cannot do without.
 */
function canOpen(source: string, at: number, mark: string): boolean {
  const after = source[at + mark.length];
  if (after === undefined || /\s/.test(after)) return false;
  if (mark[0] !== "_") return true;
  return !isWordChar(source[at - 1] ?? "");
}

/** The mirror of `canOpen`: the text before a closer must not be a space. */
function canClose(source: string, at: number, mark: string): boolean {
  if (/\s/.test(source[at - 1] ?? " ")) return false;
  if (mark[0] !== "_") return true;
  return !isWordChar(source[at + mark.length] ?? "");
}

/** Where `mark` closes, or -1. Code spans and escapes are skipped over. */
function findClose(source: string, from: number, mark: string): number {
  let at = from;
  while (at < source.length) {
    if (source[at] === "\\") {
      at += 2;
      continue;
    }
    if (source[at] === "`") {
      const span = codeSpan(source, at);
      if (span) {
        at = span.end;
        continue;
      }
    }
    if (source.startsWith(mark, at)) {
      const run = runLength(source, at, mark[0]);
      // Exactly this many: `*` does not close inside the `**` of
      // `*a **b** c*`, and `**` does not close inside a `***`.
      if (run === mark.length && canClose(source, at, mark)) return at;
      at += run;
      continue;
    }
    at += 1;
  }
  return -1;
}

/**
 * The code span starting at `at`, closed by a backtick run of the same length.
 *
 * Matching the length is what lets a span hold backticks: ``` ``a ` b`` ```.
 */
function codeSpan(
  source: string,
  at: number,
): { fence: string; text: string; end: number } | null {
  const length = runLength(source, at, "`");
  const fence = source.slice(at, at + length);
  let from = at + length;
  while (from < source.length) {
    if (source[from] !== "`") {
      from += 1;
      continue;
    }
    const run = runLength(source, from, "`");
    if (run === length) {
      return { fence, text: source.slice(at + length, from), end: from + run };
    }
    from += run;
  }
  return null;
}

/** How many `char` in a row start at `at`. */
function runLength(source: string, at: number, char: string): number {
  let length = 0;
  while (source[at + length] === char) length += 1;
  return length;
}

const PUNCTUATION = new Set([..."\\`*_~[]()#+-.!<>{}|\"'$%&,/:;=?@^"]);

const isPunctuation = (char: string): boolean => PUNCTUATION.has(char);

const isWordChar = (char: string): boolean => /[\p{L}\p{N}]/u.test(char);

/** Fold neighbours that came out identical, and drop the empty ones. */
function merge(runs: Run[]): Run[] {
  const merged: Run[] = [];
  for (const run of runs) {
    if (!run.text) continue;
    const last = merged[merged.length - 1];
    if (
      last &&
      last.bold === run.bold &&
      last.italic === run.italic &&
      last.code === run.code &&
      last.strike === run.strike
    ) {
      last.text += run.text;
      continue;
    }
    merged.push({ ...run });
  }
  return merged;
}
