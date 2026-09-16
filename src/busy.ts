// The spinner in the bottom-right corner, shown while something is loading.
//
// A counter rather than a flag: a project open and a project-list read can be in
// flight at once, and the spinner should stay until the last of them is done.
// The element lives outside `#app`, which is cleared on every render.

let inFlight = 0;
let node: HTMLElement | null = null;

function indicator(): HTMLElement {
  if (!node) {
    node = document.createElement("div");
    node.className = "busy";
    node.setAttribute("aria-hidden", "true");
    document.body.append(node);
  }
  return node;
}

/** Keep the spinner up until `work` settles, however it settles. */
export async function whileBusy<T>(work: Promise<T>): Promise<T> {
  inFlight += 1;
  indicator().classList.add("busy--on");
  try {
    return await work;
  } finally {
    inFlight -= 1;
    if (inFlight === 0) indicator().classList.remove("busy--on");
  }
}
