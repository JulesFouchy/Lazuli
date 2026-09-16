// Inline Markdown, parsed once for both the things that draw it.
//
// `renderInline` builds DOM for the timeline, the viewer and the project name;
// `video.ts` paints the same runs on the export canvas. Sharing the parse is
// what stops the page and the video drifting apart.
//
// **Inline only** — bold, italic, code, strikethrough. Not headings, lists or
// blockquotes, and the reason is the video rather than the page. Timeline cards
// are HTML and could render anything; frames are drawn on a `<canvas>`, which
// has no layout engine, so a block element would mean hand-implementing bullets,
// indentation and margin collapsing inside the frame renderer. Inline runs are
// tractable because each one is a `ctx.font` variant measured with `measureText`.
// Rendering formatting on the timeline and silently dropping it from the export
// would be worse than not offering it at all — the export is meant to be what
// you saw.
//
// Hand-rolled rather than `marked`: the whole grammar is four delimiters, where
// a CommonMark library would have to be argued out of every block construct it
// knows, and each one it produced would be a hole in the frame renderer. The
// cost is the flanking rules below, which are the only subtle part — and they
// matter here more than the rest of CommonMark does, because `2 * 3` and
// `entry_editor.ts` are things people write in a note.

/** A stretch of text carrying one set of emphases. */
export interface Run {
  text: string;
  bold: boolean;
  italic: boolean;
  code: boolean;
  strike: boolean;
}

type Style = Omit<Run, "text">;

const PLAIN: Style = {
  bold: false,
  italic: false,
  code: false,
  strike: false,
};

/**
 * The delimiters, longest first so that `**` is never read as two `*` and
 * `***` is never read as `**` followed by a stray one.
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

/** The source as a flat list of styled runs, in order. */
export function parseInline(source: string): Run[] {
  return merge(scan(source, PLAIN));
}

/** The text without its markers: for an `alt`, a title, a filename, a toast. */
export function plainText(source: string): string {
  return parseInline(source)
    .map((run) => run.text)
    .join("");
}

/**
 * Whether the source and what it renders to are different text.
 *
 * Which is the question "is there any markup here at all" — a name or a note
 * with none can be shown and edited as the same string, and the places that
 * swap between the two use this to leave the common case alone.
 */
export function hasMarkup(source: string): boolean {
  return plainText(source) !== source;
}

/** The runs as DOM. Empty source gives an empty fragment. */
export function renderInline(source: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  for (const run of parseInline(source)) fragment.append(asElement(run));
  return fragment;
}

/**
 * One run, wrapped in the tags its emphases call for.
 *
 * `code` goes innermost, so emphasis around a code span reads as emphasis on
 * the span rather than as a bolder monospace.
 */
function asElement(run: Run): Node {
  const wrappers = [
    [run.code, "code"],
    [run.italic, "em"],
    [run.strike, "s"],
    [run.bold, "strong"],
  ] as const;

  let node: Node = document.createTextNode(run.text);
  for (const [wanted, tag] of wrappers) {
    if (!wanted) continue;
    const element = document.createElement(tag);
    element.append(node);
    node = element;
  }
  return node;
}

/** Walk `source`, emitting runs; recurses once per nested emphasis. */
function scan(source: string, style: Style): Run[] {
  const runs: Run[] = [];
  let literal = "";
  const flush = (): void => {
    if (literal) runs.push({ text: literal, ...style });
    literal = "";
  };

  let at = 0;
  while (at < source.length) {
    const char = source[at];

    // A backslash before punctuation is that punctuation: the way to write
    // about asterisks in a note.
    if (char === "\\" && isPunctuation(source[at + 1] ?? "")) {
      literal += source[at + 1];
      at += 2;
      continue;
    }

    if (char === "`") {
      const span = codeSpan(source, at);
      if (span) {
        flush();
        // Nothing inside a code span is a delimiter, so it is taken whole and
        // not scanned. Backslashes in it are backslashes.
        runs.push({ text: span.text, ...style, code: true });
        at = span.end;
        continue;
      }
    }

    const emphasis = openDelimiter(source, at);
    if (emphasis) {
      flush();
      runs.push(...scan(emphasis.inner, { ...style, ...emphasis.adds }));
      at = emphasis.end;
      continue;
    }

    literal += char;
    at += 1;
  }
  flush();
  return runs;
}

/** The emphasis starting at `at`, if one both opens and closes there. */
function openDelimiter(
  source: string,
  at: number,
): { inner: string; adds: Partial<Style>; end: number } | null {
  for (const { mark, adds } of DELIMITERS) {
    if (!source.startsWith(mark, at)) continue;
    if (!canOpen(source, at, mark)) continue;
    const close = findClose(source, at + mark.length, mark);
    // Nothing between the two is not an emphasis, it is a pair of literal
    // marks: without this, the leading `*` of an unclosed `**bold` closes on
    // the second one and eats them both.
    if (close <= at + mark.length) continue;
    return {
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
): { text: string; end: number } | null {
  const fence = runLength(source, at, "`");
  let from = at + fence;
  while (from < source.length) {
    if (source[from] !== "`") {
      from += 1;
      continue;
    }
    const run = runLength(source, from, "`");
    if (run === fence) return { text: source.slice(at + fence, from), end: from + run };
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
