// Who a synced project is shared with, and how somebody else gets in.
//
// There is no access control here to write. Drive enforces reader, writer and
// owner on its own servers, so this is a screen over its permissions and the
// roles it shows are the ones that are actually true — see `drive.rs`.

import type { Member } from "./api";
import {
  projectMembers,
  setMyNameHere,
  shareProject,
  unshareProject,
} from "./api";
import { el, toast, toastError } from "./ui";

/** How a Drive role reads to somebody who has never seen Drive's console. */
function roleReads(role: string): string {
  switch (role) {
    case "owner":
      return "Owner";
    case "writer":
      return "Can write";
    case "commenter":
    case "reader":
      return "Can read";
    default:
      return role;
  }
}

/**
 * The people section of the Syncing dialog.
 *
 * Only for a project that syncs: there is nobody to share a folder that does
 * not exist with, and saying so is better than an empty list.
 */
export interface MembersSection {
  node: HTMLElement;
  /** Re-read whether the project syncs, and who is in it. */
  refresh: () => void;
}

export function membersSection(path: string, isOn: () => boolean): MembersSection {
  const list = el("div", { class: "members" });
  const state = el("p", { class: "hint", text: "…" });

  const email = el("input", {
    class: "input",
    type: "email",
    placeholder: "Their Google account",
  }) as HTMLInputElement;

  const role = el("select", { class: "input" }) as HTMLSelectElement;
  role.append(
    el("option", { value: "writer", text: "Can write" }),
    el("option", { value: "reader", text: "Can read" }),
  );

  const invite = el("button", { class: "button", text: "Invite" });

  const nameHere = el("input", {
    class: "input",
    type: "text",
    placeholder: "Your name, in this project only",
  }) as HTMLInputElement;

  const section = el(
    "div",
    { class: "field" },
    el("label", { text: "People" }),
    state,
    list,
    el("div", { class: "row" }, email, role, invite),
    el(
      "div",
      { class: "field" },
      el("label", { text: "How you appear here" }),
      nameHere,
      el("p", {
        class: "hint",
        text: "Left empty, your own name shows. Filled in, it is used in this project and nowhere else.",
      }),
    ),
  );

  const paint = (members: Member[]) => {
    list.replaceChildren(
      ...members.map((member) =>
        el(
          "div",
          { class: "members__row" },
          el(
            "div",
            { class: "members__who" },
            el("span", { text: member.name || member.email || "Someone" }),
            el("span", { class: "members__email", text: member.email }),
          ),
          el("span", { class: "members__role", text: roleReads(member.role) }),
          // The owner is not removable, by Drive and by sense.
          member.role === "owner"
            ? el("span", { class: "members__role", text: "" })
            : el("button", {
                class: "button button--ghost button--danger",
                text: "Remove",
                onclick: () => void act(unshareProject(path, member.id), "Could not remove them"),
              }),
        ),
      ),
    );
    state.textContent =
      members.length <= 1
        ? "Only you. Invite somebody and Google sends them the folder — they need no account of Lazuli's, only a Google one."
        : "Google enforces these, not Lazuli. Anyone who can write can add entries and edit any of them.";
  };

  const act = async (call: Promise<Member[]>, whenItFails: string) => {
    invite.disabled = true;
    try {
      paint(await call);
    } catch (err) {
      toastError(whenItFails, err);
    }
    invite.disabled = false;
  };

  invite.onclick = () => {
    const address = email.value.trim();
    if (!address) return;
    void act(shareProject(path, address, role.value), "Could not share it").then(() => {
      email.value = "";
      // Said because Google sends the mail, not us: nothing else on screen
      // would tell them an invitation went anywhere.
      toast(`Invited ${address}. Google has emailed them the folder.`);
    });
  };

  // Saved on blur, like the name in the profile dialog: a name is finished in
  // one go, and a write per keystroke would republish into the project each time.
  nameHere.addEventListener("blur", () => {
    void setMyNameHere(nameHere.value.trim() || null).catch((err) =>
      toastError("Could not change how you appear here", err),
    );
  });

  // Asked again rather than answered once: the dialog is built before its own
  // status has come back, and the user can turn syncing on while looking at it.
  // Deciding at construction meant it always said "sync this first", however
  // long the project had been syncing.
  const refresh = () => {
    const on = isOn();
    list.hidden = !on;
    invite.parentElement?.toggleAttribute("hidden", !on);
    nameHere.parentElement?.toggleAttribute("hidden", !on);
    if (!on) {
      state.textContent =
        "Sync this project first. Sharing is the Drive folder's, so there has to be one.";
      return;
    }
    void projectMembers(path)
      .then(paint)
      .catch(() => {
        state.textContent = "Could not read who this is shared with.";
      });
  };
  refresh();

  return { node: section, refresh };
}
