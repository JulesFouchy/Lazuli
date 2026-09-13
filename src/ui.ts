// Small DOM helpers and the toast queue.

type Attrs = Record<string, string | number | boolean | EventListener | null>;
type Child = Node | string | null | undefined | false;

/**
 * Build an element. Keys starting with `on` are treated as listeners, `class`
 * and `text` are spelled out, everything else becomes an attribute.
 */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === false) continue;
    if (key.startsWith("on") && typeof value === "function") {
      node.addEventListener(key.slice(2).toLowerCase(), value as EventListener);
    } else if (key === "text") {
      node.textContent = String(value);
    } else if (key === "class") {
      node.className = String(value);
    } else {
      node.setAttribute(key, String(value));
    }
  }
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child);
  }
  return node;
}

export function clear(node: HTMLElement): HTMLElement {
  node.replaceChildren();
  return node;
}

const TOAST_MS = 6000;

export interface ToastOptions {
  /** A single inline action, typically "Undo". */
  action?: { label: string; run: () => void };
  error?: boolean;
  /** Milliseconds on screen. */
  duration?: number;
  /** Called when it leaves the screen, however it came to go. */
  onGone?: () => void;
}

/**
 * Show a transient message, returning the way to take it back down.
 *
 * Deletes are not confirmed with a dialog, so this is where the undo lives: the
 * toast has to stay long enough to be a real safety net, and stays clickable
 * for cases where Ctrl+Z would be swallowed by a focused text field. An offer
 * taken up elsewhere — Ctrl+Z rather than the button — dismisses it, so what is
 * on screen is only ever what can still be done.
 */
export function toast(message: string, options: ToastOptions = {}): () => void {
  const host = document.getElementById("toasts");
  if (!host) return () => {};

  const node = el(
    "div",
    { class: `toast${options.error ? " toast--error" : ""}`, role: "status" },
    el("span", { class: "toast__message", text: message }),
    options.action &&
      el("button", {
        class: "toast__action",
        text: options.action.label,
        onclick: () => {
          options.action?.run();
          dismiss();
        },
      }),
  );

  let timer: ReturnType<typeof setTimeout> | undefined;
  let gone = false;
  const dismiss = () => {
    if (gone) return;
    gone = true;
    clearTimeout(timer);
    node.remove();
    options.onGone?.();
  };

  host.append(node);
  timer = setTimeout(dismiss, options.duration ?? TOAST_MS);
  // Keep it up while the pointer is on it, so a slow reader can still undo.
  node.addEventListener("mouseenter", () => clearTimeout(timer));
  node.addEventListener("mouseleave", () => {
    timer = setTimeout(dismiss, 1200);
  });
  return dismiss;
}

/** Report a failed command without swallowing the cause. */
export function toastError(context: string, err: unknown): void {
  const detail = typeof err === "string" ? err : String(err);
  toast(`${context}: ${detail}`, { error: true, duration: 10000 });
}

/** Whether the focus is somewhere that owns its own keystrokes. */
export function isEditing(): boolean {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return false;
  return (
    active.isContentEditable ||
    active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement
  );
}
