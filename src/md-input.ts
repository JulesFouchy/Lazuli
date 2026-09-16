// A text field that shows its own Markdown as you write it.
//
// Used by the note in the entry editor and by the project's name. Both hold
// Markdown source, both should show what it does while it is being typed, and
// the two would otherwise have grown separate machinery for the same thing.
//
// **Why not a textarea with a highlighted layer behind it.** That is the usual
// trick — a mirror `div` under a transparent `textarea` — and it cannot work
// here. It only holds while the styling leaves the glyphs where they were, so
// it is fine for colour and wrong for weight and slant: bold Inter is wider
// than regular, and one bold word early in a line slides every character after
// it out from under the caret. Showing bold means the text itself carries the
// style, which means one editable element with markup inside it.
//
// So: `contenteditable="plaintext-only"`, which is a plain-text field the
// browser will not put its own markup into — Enter inserts a newline, paste
// arrives as text, and the inline styling below is ours alone. On every
// keystroke the content is re-highlighted and the caret put back where it was,
// by character offset; see `caretOffset`.

import { highlight } from "./markdown";
import { el } from "./ui";

export interface MarkdownInputOptions {
  /**
   * The element to make editable. `div` unless something else is wanted for
   * what the text *is* — the project's name is the page's `h1` as well as a
   * field, and an editable element keeps whatever role its tag gave it.
   */
  tag?: "div" | "h1";
  /** Class on the field itself, on top of `md-input`. */
  class?: string;
  /** Shown while the field is empty. */
  placeholder?: string;
  /** One line, so Enter is not a newline and is left to `onKeydown`. */
  singleLine?: boolean;
  onInput?: (value: string) => void;
  onKeydown?: (event: KeyboardEvent) => void;
  onBlur?: (value: string) => void;
  onFocus?: () => void;
}

export interface MarkdownInput {
  node: HTMLElement;
  /** The Markdown source. */
  value: () => string;
  /**
   * Replace the source. Keeps the caret where it was if the field has it, so
   * this is safe to call from a rescan that landed mid-sentence.
   */
  setValue: (source: string) => void;
  /** Draw `source` without making it the value — for a blurred preview. */
  showInstead: (content: Node) => void;
  /** Put the field back to showing its own value, highlighted. */
  showValue: () => void;
  focus: () => void;
}

/**
 * How long a run of keystrokes counts as one thing to undo.
 *
 * Typing a word and pressing Ctrl+Z should take the word, not the last letter
 * of it, so consecutive edits inside this window collapse into one step.
 */
const UNDO_COALESCE_MS = 500;

/** Cap on the undo stack, so a long editing session does not grow without bound. */
const MAX_UNDO = 200;

interface Snapshot {
  source: string;
  caret: number;
}

export function markdownInput(
  options: MarkdownInputOptions = {},
): MarkdownInput {
  let source = "";

  // No `role="textbox"`: a browser exposes a `contenteditable` as editable text
  // on its own, and saying it here would take the heading role off the `h1`.
  const node = el(options.tag ?? "div", {
    class: options.class ? `md-input ${options.class}` : "md-input",
    contenteditable: "plaintext-only",
    "aria-multiline": String(!options.singleLine),
    "data-placeholder": options.placeholder ?? "",
  }) as HTMLElement;

  /** True while an IME is composing, when the DOM must be left alone. */
  let composing = false;

  const isFocused = (): boolean => document.activeElement === node;

  /** Re-highlight, and put the caret back where it was — or where asked. */
  const paint = (caret?: number): void => {
    const at = caret ?? (isFocused() ? caretOffset(node) : null);
    node.replaceChildren(highlight(source));
    if (at !== null && at !== undefined) placeCaret(node, at);
  };

  // --- undo ---------------------------------------------------------------
  //
  // The browser's own undo does not survive this field: replacing its contents
  // on every keystroke throws away the nodes its undo transactions refer to,
  // and Ctrl+Z goes from "take back that word" to doing nothing at all. So the
  // field keeps its own stack of what it held, and claims Ctrl+Z.

  const undoStack: Snapshot[] = [];
  const redoStack: Snapshot[] = [];
  let lastPushAt = 0;

  /** Record what the field held before an edit, unless that edit joins the last. */
  const pushUndo = (was: Snapshot, becomes: string): void => {
    const now = Date.now();
    // A space or a newline ends a word, and a word is the unit worth taking
    // back — so a keystroke that types one always starts a new step.
    const boundary = /\s/.test(becomes.slice(was.source.length)) ||
      becomes.length < was.source.length;
    if (now - lastPushAt >= UNDO_COALESCE_MS || boundary) {
      undoStack.push(was);
      if (undoStack.length > MAX_UNDO) undoStack.shift();
    }
    lastPushAt = now;
    // Anything typed after an undo is a new future, so the old one goes.
    redoStack.length = 0;
  };

  const restore = (to: Snapshot, onto: Snapshot[]): void => {
    onto.push({ source, caret: isFocused() ? caretOffset(node) : source.length });
    source = to.source;
    node.classList.toggle("md-input--empty", source.length === 0);
    paint(to.caret);
    // The same notification a keystroke sends, so whatever saves this field
    // saves an undo too.
    options.onInput?.(source);
    // A step is one edit however long the burst that made it was.
    lastPushAt = 0;
  };

  node.addEventListener("compositionstart", () => {
    composing = true;
  });
  node.addEventListener("compositionend", () => {
    composing = false;
    read();
  });

  const read = (): void => {
    // `textContent` rather than `innerText`: the field is one `pre-wrap` box
    // with only inline markup in it, so the two agree about the text and only
    // `textContent` agrees about the newlines.
    source = node.textContent ?? "";
    node.classList.toggle("md-input--empty", source.length === 0);
    options.onInput?.(source);
  };

  node.addEventListener("input", () => {
    if (composing) return;
    const was = { source, caret: caretOffset(node) };
    const becomes = node.textContent ?? "";
    pushUndo(was, becomes);
    read();
    // Re-styling on every keystroke is what makes this a preview rather than a
    // thing that catches up when you stop. The field is a sentence, so there is
    // no amount of text here that makes it worth debouncing.
    paint();
  });

  node.addEventListener("keydown", (event) => {
    if (options.singleLine && event.key === "Enter") event.preventDefault();

    const ctrl = event.ctrlKey || event.metaKey;
    const redo =
      (ctrl && event.key === "y") ||
      (ctrl && event.shiftKey && event.key.toLowerCase() === "z");
    if (redo) {
      event.preventDefault();
      const next = redoStack.pop();
      if (next) restore(next, undoStack);
      return;
    }
    if (ctrl && !event.shiftKey && event.key === "z") {
      event.preventDefault();
      const previous = undoStack.pop();
      if (previous) restore(previous, redoStack);
      return;
    }

    options.onKeydown?.(event);
  });

  node.addEventListener("focus", () => options.onFocus?.());
  node.addEventListener("blur", () => options.onBlur?.(source));

  return {
    node,
    value: () => source,
    setValue: (next) => {
      if (next === source) return;
      source = next;
      node.classList.toggle("md-input--empty", source.length === 0);
      paint();
    },
    showInstead: (content) => {
      node.replaceChildren(content);
    },
    showValue: () => {
      node.replaceChildren(highlight(source));
    },
    focus: () => node.focus(),
  };
}

/**
 * How many characters of `field` are before the caret.
 *
 * The field's markup changes on every keystroke, so a caret cannot be held as
 * a node and an offset into it — that node is about to be replaced. A count of
 * characters survives, because the highlighting never changes the text.
 */
export function caretOffset(field: HTMLElement): number {
  const selection = window.getSelection();
  const length = (field.textContent ?? "").length;
  if (!selection || selection.rangeCount === 0) return length;
  const caret = selection.getRangeAt(0);
  if (!field.contains(caret.endContainer)) return length;
  const range = document.createRange();
  range.selectNodeContents(field);
  range.setEnd(caret.endContainer, caret.endOffset);
  return range.toString().length;
}

/** Put the caret `at` characters into `field`, counting through its markup. */
export function placeCaret(field: HTMLElement, at: number): void {
  const walker = document.createTreeWalker(field, NodeFilter.SHOW_TEXT);
  let remaining = at;
  let last: Text | null = null;

  while (walker.nextNode()) {
    const text = walker.currentNode as Text;
    if (remaining <= text.length) {
      collapse(text, remaining);
      return;
    }
    remaining -= text.length;
    last = text;
  }

  // Past the end, which happens when the caret was after the last character:
  // land on the end of the last text there is, or on the field if it is empty.
  if (last) collapse(last, last.length);
  else collapseToEnd(field);
}

function collapse(text: Text, offset: number): void {
  const range = document.createRange();
  range.setStart(text, Math.min(offset, text.length));
  range.collapse(true);
  select(range);
}

function collapseToEnd(field: HTMLElement): void {
  const range = document.createRange();
  range.selectNodeContents(field);
  range.collapse(false);
  select(range);
}

function select(range: Range): void {
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}
