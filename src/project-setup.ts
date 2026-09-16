// Creating a project, and changing its start date afterwards.
//
// Both dialogs are about the same two facts — what the project is called and
// which day is Day 1 — so they live together and word them the same way.

import { open as openDialog } from "@tauri-apps/plugin-dialog";

import type { Project } from "./api";
import {
  createProject,
  defaultProjectsDir,
  journalToday,
  newProjectTarget,
  setStartDate,
} from "./api";
import { whileBusy } from "./busy";
import { daysBetween, formatRealWorld } from "./dates";
import { openModal } from "./modal";
import { el, focusWhenActive, toastError } from "./ui";

// --- creating a project ----------------------------------------------------

/**
 * Ask for a name, a place to put it, and a start date, then create the folder.
 *
 * The folder is created rather than chosen: picking an existing empty folder
 * meant making one in the file dialog first, and offered the app a folder with
 * someone else's files in it as the normal case rather than the mistake.
 */
export function openNewProjectDialog(onCreated: (project: Project) => void): void {
  const nameInput = el("input", {
    class: "input",
    type: "text",
    // A label, not an example: the example read as a suggestion of what kind
    // of thing a project is supposed to be.
    placeholder: "Project name",
  }) as HTMLInputElement;

  // Typed as well as browsed: pasting a path is often quicker than walking a
  // folder tree, and a path that does not exist yet is fine — the folder is
  // created either way, and the line underneath says where it will land.
  const locationInput = el("input", {
    class: "input card__grow",
    type: "text",
    spellcheck: "false",
  }) as HTMLInputElement;

  const dateInput = el("input", {
    class: "input input--date",
    type: "date",
  }) as HTMLInputElement;

  const destination = el("p", { class: "hint" });
  const createButton = el("button", {
    class: "button button--primary",
    text: "Create project",
  }) as HTMLButtonElement;

  /** The folder we last confirmed is free, or null while anything is wrong. */
  let target: string | null = null;
  // Every keystroke asks Rust where the project would land; a stale answer
  // arriving late must not overwrite a newer one.
  let latestRequest = 0;

  const refreshDestination = () => {
    const name = nameInput.value.trim();
    const parent = locationInput.value;
    if (!name || !parent) {
      target = null;
      createButton.disabled = true;
      destination.textContent = name
        ? "Choose where to keep it."
        : "Give the project a name.";
      destination.classList.remove("hint--problem");
      return;
    }

    const request = ++latestRequest;
    void newProjectTarget(parent, name)
      .then(({ path, problem }) => {
        if (request !== latestRequest) return;
        target = problem === null ? path : null;
        createButton.disabled = problem !== null;
        destination.textContent = problem ?? `Creates ${path}`;
        destination.classList.toggle("hint--problem", problem !== null);
      })
      .catch((err) => {
        if (request !== latestRequest) return;
        target = null;
        createButton.disabled = true;
        destination.textContent = String(err);
        destination.classList.add("hint--problem");
      });
  };

  nameInput.addEventListener("input", refreshDestination);
  locationInput.addEventListener("input", refreshDestination);
  for (const field of [nameInput, locationInput]) {
    field.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && !createButton.disabled) create();
    });
  }

  const browse = async () => {
    const chosen = await openDialog({
      directory: true,
      title: "Where should the project folder go?",
      defaultPath: locationInput.value || (await defaultProjectsDir()),
    });
    if (typeof chosen !== "string") return;
    locationInput.value = chosen;
    refreshDestination();
  };

  const create = () => {
    const parent = locationInput.value;
    const name = nameInput.value.trim();
    if (!target || !parent || !name || !dateInput.value) return;
    createButton.disabled = true;
    void (async () => {
      try {
        // The caller closes this dialog, as part of recording the move to the
        // new project; closing it here would record an extra step back to the
        // launch screen on the way.
        onCreated(
          await whileBusy(createProject(parent, name, dateInput.value)),
        );
      } catch (err) {
        toastError("Could not create the project", err);
        createButton.disabled = false;
      }
    })();
  };

  createButton.addEventListener("click", create);

  openModal({
    title: "New project",
    body: el(
      "div",
      { class: "modal__body-inner" },
      el("div", { class: "field" }, el("label", { text: "Name" }), nameInput),
      el(
        "div",
        { class: "field" },
        el("label", { text: "Location" }),
        el(
          "div",
          { class: "row" },
          locationInput,
          el("button", {
            class: "button",
            text: "Browse…",
            onclick: () => void browse(),
          }),
        ),
      ),
      el(
        "div",
        { class: "field" },
        el("label", { text: "Start date" }),
        dateInput,
        el("p", {
          class: "hint",
          // The point of overriding it: the journal usually starts before the
          // day you got around to making the folder.
          text: "Day 1. Set it back to the day the work really started.",
        }),
      ),
      destination,
    ),
    foot: el(
      "div",
      { class: "modal__foot" },
      el("span", { class: "card__grow" }),
      createButton,
    ),
  });

  createButton.disabled = true;
  focusWhenActive(nameInput);

  // Both defaults need a round trip; fill them in as they arrive rather than
  // holding the dialog closed until they do.
  void defaultProjectsDir().then((dir) => {
    if (!locationInput.value) {
      locationInput.value = dir;
      refreshDestination();
    }
  });
  void journalToday().then((today) => {
    if (!dateInput.value) dateInput.value = today;
  });
}

// --- changing the start date afterwards ------------------------------------

/**
 * Edit an open project's Day 1.
 *
 * Written straight through on change, like the entry editor: there is no save
 * button anywhere in the app, and the rescan puts the renumbered timeline on
 * screen immediately.
 */
export function openStartDateEditor(project: Project): void {
  const dateInput = el("input", {
    class: "input input--date",
    type: "date",
    value: project.meta.start_date,
  }) as HTMLInputElement;

  const effect = el("p", { class: "hint" });

  const describe = () => {
    const start = dateInput.value;
    if (!start) {
      effect.textContent = "";
      effect.classList.remove("hint--problem");
      return;
    }
    if (project.entries.length === 0) {
      effect.textContent = `Day 1 is ${formatRealWorld(start)}.`;
      effect.classList.remove("hint--problem");
      return;
    }

    const latest = project.entries[project.entries.length - 1];
    const earliest = project.entries[0];
    // The same arithmetic Rust does on the next scan; shown here so the effect
    // of a date is visible before it is committed to.
    const before = project.entries.filter(
      (entry) => daysBetween(start, entry.journal_date) < 0,
    ).length;

    effect.textContent =
      `Your latest entry becomes Day ${daysBetween(start, latest.journal_date) + 1}.` +
      (before > 0
        ? ` ${before === 1 ? "1 entry falls" : `${before} entries fall`}` +
          ` before this date, down to Day ${daysBetween(start, earliest.journal_date) + 1}.`
        : "");
    effect.classList.toggle("hint--problem", before > 0);
  };

  dateInput.addEventListener("input", describe);
  dateInput.addEventListener("change", () => {
    if (!dateInput.value) return;
    void setStartDate(dateInput.value).catch((err) =>
      toastError("Could not change the start date", err),
    );
  });
  describe();

  openModal({
    title: "Start date",
    body: el(
      "div",
      { class: "modal__body-inner" },
      el(
        "div",
        { class: "field" },
        el("label", { text: "Day 1" }),
        dateInput,
        el("p", {
          class: "hint",
          text: "A journal date: an entry written at 01:00 counts as the day before.",
        }),
      ),
      effect,
    ),
  });

  focusWhenActive(dateInput);
}
