// Video export: composing frames on a canvas and feeding them to ffmpeg.
//
// One frame per entry, held a second each. Frames never touch disk — each PNG
// goes straight to ffmpeg's stdin — so a project with thousands of entries
// exports in flat memory and flat disk.

import type { Entry, Project } from "./api";
import {
  assetUrl,
  exportBegin,
  exportCancel,
  exportFinish,
  exportPushFrame,
} from "./api";
import { formatRealWorld } from "./dates";
import { accentOnDark } from "./theme";

export interface ExportSettings {
  output: string;
  secondsPerFrame: number;
  width: number;
  height: number;
  fps: number;
}

export const DEFAULT_SETTINGS: Omit<ExportSettings, "output"> = {
  secondsPerFrame: 1,
  width: 1920,
  height: 1080,
  fps: 30,
};

/** The face bundled with the app, so exports look the same on any machine. */
const MONO = "LazuliMono";
const SANS = "LazuliSans";

export interface ExportProgress {
  done: number;
  total: number;
}

/** Only entries with a chosen image become frames. */
export const exportableEntries = (project: Project): Entry[] =>
  project.entries.filter((entry) => entry.image !== null);

/**
 * Render every entry and encode them into a video.
 *
 * Frames are pushed one at a time and each push waits for ffmpeg to take it,
 * so ffmpeg's own backpressure paces this loop rather than the whole video
 * being buffered up first.
 */
export async function exportVideo(
  project: Project,
  settings: ExportSettings,
  onProgress: (progress: ExportProgress) => void,
  shouldCancel: () => boolean,
): Promise<string> {
  const entries = exportableEntries(project);
  if (entries.length === 0) {
    throw new Error("No entry has a chosen image, so there is nothing to show.");
  }

  // The fonts must be resident before the first `fillText`, or early frames
  // silently render in a fallback face while later ones do not.
  await Promise.all([
    document.fonts.load(`700 48px ${MONO}`),
    document.fonts.load(`400 48px ${MONO}`),
    document.fonts.load(`500 48px ${SANS}`),
  ]);

  const canvas = document.createElement("canvas");
  canvas.width = settings.width;
  canvas.height = settings.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("This machine cannot provide a 2D canvas.");

  await exportBegin({
    output: settings.output,
    seconds_per_frame: settings.secondsPerFrame,
    width: settings.width,
    height: settings.height,
    fps: settings.fps,
  });

  try {
    for (const [index, entry] of entries.entries()) {
      if (shouldCancel()) {
        await exportCancel();
        throw new Error("Export cancelled.");
      }
      await drawFrame(ctx, canvas, project, entry);
      await exportPushFrame(await canvasToPng(canvas));
      onProgress({ done: index + 1, total: entries.length });
    }
    return await exportFinish();
  } catch (err) {
    // Leaves no half-written mp4 behind, whatever went wrong.
    await exportCancel().catch(() => {});
    throw err;
  }
}

function canvasToPng(canvas: HTMLCanvasElement): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (!blob) {
        reject(new Error("The canvas produced no image data."));
        return;
      }
      blob
        .arrayBuffer()
        .then((buffer) => resolve(new Uint8Array(buffer)))
        .catch(reject);
    }, "image/png");
  });
}

const imageCache = new Map<string, HTMLImageElement>();

function loadImage(url: string): Promise<HTMLImageElement> {
  const cached = imageCache.get(url);
  if (cached) return Promise.resolve(cached);
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      // One entry, one frame: caching only guards against a repeat export.
      if (imageCache.size > 8) imageCache.clear();
      imageCache.set(url, image);
      resolve(image);
    };
    image.onerror = () => reject(new Error(`Could not load ${url}`));
    image.src = url;
  });
}

async function drawFrame(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  project: Project,
  entry: Entry,
): Promise<void> {
  const { width, height } = canvas;
  const image = await loadImage(
    assetUrl(project.root, "entries", entry.id, entry.image ?? ""),
  );

  ctx.clearRect(0, 0, width, height);

  // Where the aspect ratio does not match, fill behind with a blurred, scaled
  // copy rather than letterboxing to black: a portrait photo in a 16:9 frame
  // then still fills it.
  drawCover(ctx, image, 0, 0, width, height, true);
  drawContain(ctx, image, width, height);

  drawScrims(ctx, width, height);
  drawLabels(ctx, width, height, entry);
  drawSentence(ctx, width, height, entry.text);
}

/** Fill the rect, cropping the overflow. Optionally blurred, for a backdrop. */
function drawCover(
  ctx: CanvasRenderingContext2D,
  image: HTMLImageElement,
  x: number,
  y: number,
  w: number,
  h: number,
  blurred: boolean,
): void {
  const scale = Math.max(w / image.width, h / image.height);
  const dw = image.width * scale;
  const dh = image.height * scale;
  ctx.save();
  if (blurred) {
    // Scaled up as well as blurred, so the blur's soft edges stay off-frame.
    ctx.filter = `blur(${Math.round(w / 40)}px) brightness(0.55)`;
    const bleed = 1.15;
    ctx.drawImage(
      image,
      x + (w - dw * bleed) / 2,
      y + (h - dh * bleed) / 2,
      dw * bleed,
      dh * bleed,
    );
  } else {
    ctx.drawImage(image, x + (w - dw) / 2, y + (h - dh) / 2, dw, dh);
  }
  ctx.restore();
}

/** Draw the whole image, centred, as large as fits. */
function drawContain(
  ctx: CanvasRenderingContext2D,
  image: HTMLImageElement,
  w: number,
  h: number,
): void {
  const scale = Math.min(w / image.width, h / image.height);
  const dw = image.width * scale;
  const dh = image.height * scale;
  ctx.drawImage(image, (w - dw) / 2, (h - dh) / 2, dw, dh);
}

/** Darken the top and bottom edges so light photos do not swallow the text. */
function drawScrims(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
): void {
  const top = ctx.createLinearGradient(0, 0, 0, height * 0.22);
  top.addColorStop(0, "rgba(0, 0, 0, 0.62)");
  top.addColorStop(1, "rgba(0, 0, 0, 0)");
  ctx.fillStyle = top;
  ctx.fillRect(0, 0, width, height * 0.22);

  const bottom = ctx.createLinearGradient(0, height * 0.62, 0, height);
  bottom.addColorStop(0, "rgba(0, 0, 0, 0)");
  bottom.addColorStop(1, "rgba(0, 0, 0, 0.8)");
  ctx.fillStyle = bottom;
  ctx.fillRect(0, height * 0.62, width, height * 0.38);
}

/**
 * `Day N` top-left, the real-world date top-right.
 *
 * Both monospaced, because at one second a frame any wobble in a label that
 * should sit still reads as a glitch. The date is zero-padded to a constant
 * twelve characters since it changes every frame. The day number is
 * deliberately *not* padded: it only widens at 9→10, 99→100 and so on, which
 * is rare enough to read as an event worth noticing rather than as jitter.
 */
function drawLabels(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  entry: Entry,
): void {
  const size = Math.round(height * 0.032);
  const margin = Math.round(height * 0.045);

  ctx.save();
  ctx.font = `700 ${size}px ${MONO}`;
  ctx.textBaseline = "top";

  // Anchored to its own corner, so the number grows rightward and the word
  // "Day" never moves.
  // The app's accent, so an export looks like the app it came from. Lightened
  // first where the chosen colour would be too dark to read on the scrim.
  ctx.fillStyle = accentOnDark();
  ctx.textAlign = "left";
  ctx.fillText(`Day ${entry.day_number}`, margin, margin);

  ctx.font = `400 ${size}px ${MONO}`;
  ctx.fillStyle = "rgba(255, 255, 255, 0.82)";
  ctx.textAlign = "right";
  ctx.fillText(formatRealWorld(entry.journal_date), width - margin, margin);
  ctx.restore();
}

/** How many lines of the sentence fit before it is cut short. */
const MAX_LINES = 3;

function drawSentence(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  text: string,
): void {
  const trimmed = text.trim();
  if (!trimmed) return;

  const size = Math.round(height * 0.042);
  const lineHeight = Math.round(size * 1.35);
  const margin = Math.round(height * 0.055);
  const maxWidth = width - margin * 2;

  ctx.save();
  ctx.font = `500 ${size}px ${SANS}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";

  const lines = wrap(ctx, trimmed.replace(/\s+/g, " "), maxWidth);
  // A long note must not push the layout off-frame, so it is cut rather than
  // allowed to grow upward without limit.
  if (lines.length > MAX_LINES) {
    lines.length = MAX_LINES;
    lines[MAX_LINES - 1] = ellipsise(ctx, lines[MAX_LINES - 1], maxWidth);
  }

  const baseline = height - margin - (lines.length - 1) * lineHeight;
  ctx.fillStyle = "#ffffff";
  ctx.shadowColor = "rgba(0, 0, 0, 0.65)";
  ctx.shadowBlur = Math.round(size * 0.5);
  lines.forEach((line, index) => {
    ctx.fillText(line, width / 2, baseline + index * lineHeight);
  });
  ctx.restore();
}

function wrap(
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
): string[] {
  const lines: string[] = [];
  let line = "";
  for (const word of text.split(" ")) {
    const candidate = line ? `${line} ${word}` : word;
    if (line && ctx.measureText(candidate).width > maxWidth) {
      lines.push(line);
      line = word;
    } else {
      line = candidate;
    }
  }
  if (line) lines.push(line);
  return lines;
}

function ellipsise(
  ctx: CanvasRenderingContext2D,
  line: string,
  maxWidth: number,
): string {
  let text = line;
  while (text && ctx.measureText(`${text}…`).width > maxWidth) {
    text = text.slice(0, -1);
  }
  return `${text.trimEnd()}…`;
}

/**
 * Render one entry at preview size, for the export dialog.
 *
 * Uses the same `drawFrame`, so what the preview shows is what the video gets.
 */
export async function renderPreview(
  project: Project,
  entry: Entry,
  width: number,
  height: number,
): Promise<HTMLCanvasElement> {
  await Promise.all([
    document.fonts.load(`700 48px ${MONO}`),
    document.fonts.load(`400 48px ${MONO}`),
    document.fonts.load(`500 48px ${SANS}`),
  ]);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("This machine cannot provide a 2D canvas.");
  await drawFrame(ctx, canvas, project, entry);
  return canvas;
}
