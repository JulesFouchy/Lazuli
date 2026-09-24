// Who you are: the round button in the corner, and the dialog behind it.
//
// The profile is global, not per project. It is edited here once and *copied*
// into each project you write in, because whoever else opens that folder can
// read the folder and nothing else of yours — see `authors.rs`.

import type { MyProfile } from "./api";
import {
  clearMyAvatar,
  myNameHere,
  myProfile,
  setMyAvatar,
  setMyName,
  setMyNameHere,
} from "./api";
import { openModal } from "./modal";
import { el, toastError } from "./ui";

/** What the last read said, so a button paints without waiting for a call. */
let known: MyProfile | null = null;

/** Read the profile once at startup, so the first paint has it. */
export async function startProfile(): Promise<void> {
  try {
    known = await myProfile();
  } catch {
    // The button falls back to the drawn figure; nothing else depends on this.
  }
}

/** Repaints the dialog on screen, while one is open. */
let repaintDialog: (() => void) | null = null;

/**
 * Take a fresh profile and repaint everything showing it.
 *
 * The buttons are found in the DOM rather than kept in a list: `render` rebuilds
 * the whole page on every change, so a list would fill up with detached nodes.
 */
export function adopt(profile: MyProfile): void {
  known = profile;
  for (const button of document.querySelectorAll<HTMLElement>(".avatar--button")) {
    paintButton(button);
  }
  repaintDialog?.();
}

/**
 * The round button: your picture, or a drawn figure when you have none.
 *
 * Round because that is what a picture of a person is everywhere else, and
 * because it tells the button apart from the square, labelled ones beside it.
 */
export function profileButton(projectName: string | null = null): HTMLElement {
  const button = el("button", {
    class: "avatar avatar--button",
    onclick: () => openProfileDialog(projectName),
  });
  paintButton(button);
  return button;
}

function paintButton(button: HTMLElement): void {
  const label = known
    ? `${known.name} — your name and picture`
    : "Your name and picture";
  button.title = label;
  button.setAttribute("aria-label", label);
  button.replaceChildren(avatarContents(known?.avatar ?? null, known?.name ?? ""));
}

/**
 * What goes inside a round avatar: the picture, or a figure drawn in line art.
 *
 * Drawn rather than an initial: an initial stands in for a name, and the name
 * is already written beside this everywhere it appears.
 */
export function avatarContents(source: string | null, alt: string): HTMLElement {
  if (source) {
    return el("img", { class: "avatar__image", src: source, alt });
  }
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", "avatar__figure");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML =
    '<circle cx="12" cy="8.5" r="3.6" fill="none" stroke="currentColor" stroke-width="1.7"/>' +
    '<path d="M4.8 19.6a7.2 7.2 0 0 1 14.4 0" fill="none" stroke="currentColor" ' +
    'stroke-width="1.7" stroke-linecap="round"/>';
  return svg as unknown as HTMLElement;
}

/** A small round avatar for somebody else, as an entry card shows it. */
export function authorAvatar(source: string | null, name: string): HTMLElement {
  return el("span", { class: "avatar avatar--tiny" }, avatarContents(source, name));
}

/**
 * `projectName` is the project on screen, when there is one: the dialog then
 * also offers what you are called there alone.
 */
export function openProfileDialog(projectName: string | null = null): void {
  openModal({
    title: "You",
    body: dialogBody(projectName),
    onClose: () => {
      repaintDialog = null;
    },
  });
  // Opened from what was last known so it is there at once, then brought up to
  // date. Anything else may have changed the profile since — another window,
  // the settings file edited by hand — and a dialog showing a stale name is one
  // that will save the stale name back.
  void myProfile()
    .then(adopt)
    .catch(() => {});
}

function dialogBody(projectName: string | null): HTMLElement {
  const picture = el("div", { class: "avatar avatar--large" });
  const remove = el("button", { class: "button button--ghost", text: "Remove picture" });

  const nameField = el("input", {
    class: "input",
    type: "text",
    placeholder: "Your name",
    value: known?.name ?? "",
  }) as HTMLInputElement;

  // A hidden file input rather than the native dialog plugin: the page needs the
  // bytes anyway to send them, and this way a picture can be dragged onto the
  // chooser as well as picked in it.
  const file = el("input", {
    type: "file",
    accept: "image/*",
    class: "profile__file",
  }) as HTMLInputElement;

  const paint = () => {
    picture.replaceChildren(avatarContents(known?.avatar ?? null, known?.name ?? ""));
    remove.hidden = !known?.avatar;
    // Not while it is being typed in, which is the same rule the entry editor's
    // note follows: what came back is this field's own last save.
    if (document.activeElement !== nameField) {
      nameField.value = known?.name ?? "";
    }
  };
  repaintDialog = paint;

  const apply = async (call: Promise<MyProfile>, whenItFails: string) => {
    try {
      adopt(await call);
    } catch (err) {
      toastError(whenItFails, err);
    }
    paint();
  };

  // Saved on blur and on Enter rather than as it is typed: a name is short and
  // finished in one go, and a write per keystroke would republish it into the
  // open project each time.
  nameField.addEventListener("blur", () => {
    if (nameField.value.trim() === (known?.name ?? "")) return;
    void apply(setMyName(nameField.value), "Could not change your name");
  });
  nameField.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      nameField.blur();
    }
  });

  remove.onclick = () =>
    void apply(clearMyAvatar(), "Could not remove the picture");

  file.addEventListener("change", () => {
    const chosen = file.files?.[0];
    file.value = "";
    if (!chosen) return;
    void chosen.arrayBuffer().then((buffer) =>
      apply(
        setMyAvatar(chosen.name, new Uint8Array(buffer)),
        "Could not use that picture",
      ),
    );
  });

  paint();

  return el(
    "div",
    { class: "modal__body-inner" },
    el(
      "div",
      { class: "profile" },
      el(
        "button",
        {
          class: "profile__picture",
          title: "Choose a picture",
          onclick: () => file.click(),
        },
        picture,
        el("span", { class: "profile__change", text: "Change" }),
      ),
      el(
        "div",
        { class: "profile__fields" },
        el("div", { class: "field" }, el("label", { text: "Name" }), nameField),
        remove,
      ),
    ),
    file,
    el("p", {
      class: "hint",
      text:
        "Your name and picture are yours rather than any project's. They are " +
        "copied into each project you write in, so whoever else opens it can " +
        "see who wrote what — and a project only you write in shows neither.",
    }),
    projectName === null ? null : nameHereField(projectName),
  );
}

/**
 * What you are called in the project on screen alone.
 *
 * The Discord model: your name everywhere until you decide otherwise, and then
 * only where you decided it. Saved on blur, like the name above, and for the
 * same reason — each save is a write into the project.
 */
function nameHereField(projectName: string): HTMLElement {
  const field = el("input", {
    class: "input",
    type: "text",
    placeholder: known?.name ?? "Your name",
  }) as HTMLInputElement;
  let saved = "";
  void myNameHere()
    .then((name) => {
      saved = name ?? "";
      if (document.activeElement !== field) field.value = saved;
    })
    .catch(() => {});

  field.addEventListener("blur", () => {
    const name = field.value.trim();
    if (name === saved) return;
    void setMyNameHere(name || null)
      .then(() => {
        saved = name;
      })
      .catch((err) => toastError("Could not change how you appear here", err));
  });
  field.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      field.blur();
    }
  });

  return el(
    "div",
    { class: "field" },
    el("label", { text: `In ${projectName}` }),
    field,
    el("p", {
      class: "hint",
      text: "Left empty, your own name shows. Filled in, it is used in this project and nowhere else.",
    }),
  );
}
