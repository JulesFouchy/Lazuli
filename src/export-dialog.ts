// The export dialog: a live preview of a real frame, then progress.

import type { Project } from "./api";
import { ffmpegStatus } from "./api";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { closeModal, openModal, replaceModalBody } from "./modal";
import { el, toast, toastError } from "./ui";
import {
  DEFAULT_SETTINGS,
  exportableEntries,
  exportVideo,
  renderPreview,
  type ExportProgress,
} from "./video";

const PREVIEW_WIDTH = 560;

export function openExportDialog(project: Project): void {
  openModal({ title: "Export summary video", body: body(project) });
}

function body(project: Project): HTMLElement {
  const entries = exportableEntries(project);
  const skipped = project.entries.length - entries.length;

  if (entries.length === 0) {
    return el("p", {
      class: "empty",
      text: "No entry has a chosen image yet, so there is nothing to show in a video.",
    });
  }

  const secondsInput = el("input", {
    class: "input",
    type: "number",
    min: "0.2",
    step: "0.1",
    value: String(DEFAULT_SETTINGS.secondsPerFrame),
  }) as HTMLInputElement;

  const sizeSelect = el(
    "select",
    { class: "input" },
    el("option", { value: "1920x1080", text: "1920 × 1080 (1080p)" }),
    el("option", { value: "2560x1440", text: "2560 × 1440 (1440p)" }),
    el("option", { value: "3840x2160", text: "3840 × 2160 (4K)" }),
    el("option", { value: "1280x720", text: "1280 × 720 (720p)" }),
    el("option", { value: "1080x1920", text: "1080 × 1920 (vertical)" }),
  ) as HTMLSelectElement;

  const summary = el("p", { class: "hint" });
  const updateSummary = () => {
    const seconds = Number(secondsInput.value) || 1;
    const total = Math.round(entries.length * seconds);
    const minutes = Math.floor(total / 60);
    const length = minutes > 0 ? `${minutes}m ${total % 60}s` : `${total}s`;
    summary.textContent =
      `${entries.length} frames, about ${length}` +
      (skipped > 0
        ? ` · ${skipped} entr${skipped === 1 ? "y" : "ies"} skipped for having no chosen image`
        : "");
  };
  secondsInput.addEventListener("input", updateSummary);
  updateSummary();

  // A real frame from the project, drawn by the same code the export uses, so
  // the preview cannot disagree with the result.
  const previewHost = el("div", { class: "preview" });
  const [width, height] = [1920, 1080];
  void renderPreview(
    project,
    entries[entries.length - 1],
    PREVIEW_WIDTH,
    Math.round((PREVIEW_WIDTH * height) / width),
  )
    .then((canvas) => previewHost.replaceChildren(canvas))
    .catch((err) => toastError("Could not draw the preview", err));

  const progressText = el("p", { class: "hint" });
  const startButton = el("button", {
    class: "button button--primary",
    text: "Export…",
  });

  let cancelled = false;

  startButton.addEventListener("click", () => {
    void (async () => {
      const ffmpeg = await ffmpegStatus();
      if (!ffmpeg) {
        toastError(
          "ffmpeg is needed to export",
          "install ffmpeg and make sure it is on your PATH, then try again",
        );
        return;
      }

      const output = await saveDialog({
        title: "Save the summary video",
        defaultPath: `${project.meta.name.replace(/[\\/:*?"<>|]/g, "-")}.mp4`,
        filters: [{ name: "MP4 video", extensions: ["mp4"] }],
      });
      if (typeof output !== "string") return;

      const [w, h] = sizeSelect.value.split("x").map(Number);
      cancelled = false;
      startButton.textContent = "Cancel";
      startButton.classList.remove("button--primary");
      const onCancel = () => {
        cancelled = true;
        progressText.textContent = "Cancelling…";
      };
      startButton.addEventListener("click", onCancel);

      try {
        const path = await exportVideo(
          project,
          {
            output,
            secondsPerFrame: Number(secondsInput.value) || 1,
            width: w,
            height: h,
            fps: DEFAULT_SETTINGS.fps,
          },
          (progress: ExportProgress) => {
            progressText.textContent = `Rendering frame ${progress.done} of ${progress.total}…`;
          },
          () => cancelled,
        );
        closeModal();
        toast(`Exported ${path}`, {
          action: { label: "Show", run: () => void revealItemInDir(path) },
          duration: 12000,
        });
      } catch (err) {
        progressText.textContent = "";
        toastError("Export failed", err);
      } finally {
        startButton.removeEventListener("click", onCancel);
        startButton.textContent = "Export…";
        startButton.classList.add("button--primary");
      }
    })();
  });

  return el(
    "div",
    { class: "export" },
    previewHost,
    el(
      "div",
      { class: "row" },
      el(
        "div",
        { class: "field" },
        el("label", { text: "Seconds per entry" }),
        secondsInput,
      ),
      el(
        "div",
        { class: "field card__grow" },
        el("label", { text: "Resolution" }),
        sizeSelect,
      ),
    ),
    summary,
    el("div", { class: "row" }, startButton, progressText),
  );
}

/** Redraw the dialog if the project changed underneath it. */
export function refreshExportDialog(project: Project): void {
  replaceModalBody(body(project));
}
