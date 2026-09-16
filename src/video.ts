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
import { parseInline, type Run } from "./markdown";
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

/**
 * Make the bundled faces resident.
 *
 * They must be loaded before the first `fillText`, or early frames silently
 * render in a fallback face while later ones do not. Every variant the frame
 * can ask for is listed: a weight or a slant asked for mid-export is a face
 * that has not arrived yet, which is the same bug one run at a time.
 *
 * A variant with no file behind it — there is no italic Inter here — resolves
 * to nothing rather than failing, and the canvas synthesises it. See `runFont`.
 */
function loadFrameFonts(): Promise<unknown> {
  return Promise.all(
    [
      `700 48px ${MONO}`,
      `400 48px ${MONO}`,
      `italic 400 48px ${MONO}`,
      `500 48px ${SANS}`,
      `600 48px ${SANS}`,
      `italic 500 48px ${SANS}`,
      `italic 600 48px ${SANS}`,
    ].map((font) => document.fonts.load(font)),
  );
}

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

  await loadFrameFonts();

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
    // Entry images are served from `asset.localhost`, which is not the page's
    // origin: drawn without this, they taint the canvas and `toBlob` throws a
    // `SecurityError` on the very first frame. Tauri's asset protocol answers
    // the CORS preflight, so asking for it is all it takes.
    image.crossOrigin = "anonymous";
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

// --- the sentence ----------------------------------------------------------
//
// The note is Markdown, so a frame is not one string in one font but a row of
// styled pieces — see `markdown.ts` for why the grammar stops at inline. Two
// things follow from that, and they are the whole of the extra machinery here:
//
// - A word can span a style change (`**bo**ld` is one word in two fonts), so
//   the unit that wrapping moves around is a word made of pieces, not a string.
// - `textAlign: "center"` centres one `fillText`, and a line is now several.
//   So the line is measured first and drawn left to right from its own start.

/** A stretch of one word in one font. */
interface Piece {
  text: string;
  font: string;
  strike: boolean;
  width: number;
}

/** A word, which must not be broken across lines however many fonts it takes. */
interface Word {
  pieces: Piece[];
  width: number;
}

/**
 * The face for one run.
 *
 * The bundled Inter has 500 and 600 and no italic at all, so bold asks for the
 * weight that exists rather than for 700, and italic is left to the canvas's
 * own synthetic oblique. A frame is a photograph with a sentence on it, not a
 * document: a slanted 500 is the right amount of emphasis here, and shipping
 * two more font files to say it is not worth the download.
 */
function runFont(run: Run, size: number): string {
  const family = run.code ? MONO : SANS;
  const weight = run.code ? (run.bold ? 700 : 400) : run.bold ? 600 : 500;
  return `${run.italic ? "italic " : ""}${weight} ${size}px ${family}`;
}

function drawSentence(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  text: string,
): void {
  const runs = parseInline(text.trim());
  if (runs.length === 0) return;

  const size = Math.round(height * 0.042);
  const lineHeight = Math.round(size * 1.35);
  const margin = Math.round(height * 0.055);
  const maxWidth = width - margin * 2;
  const base = `500 ${size}px ${SANS}`;

  ctx.save();
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";

  const space = measure(ctx, base, " ");
  const lines = wrap(toWords(ctx, runs, size), space, maxWidth);
  // A long note must not push the layout off-frame, so it is cut rather than
  // allowed to grow upward without limit.
  if (lines.length > MAX_LINES) {
    lines.length = MAX_LINES;
    ellipsise(ctx, lines[MAX_LINES - 1], base, space, maxWidth);
  }

  const baseline = height - margin - (lines.length - 1) * lineHeight;
  ctx.fillStyle = "#ffffff";
  ctx.shadowColor = "rgba(0, 0, 0, 0.65)";
  ctx.shadowBlur = Math.round(size * 0.5);
  lines.forEach((line, index) => {
    const y = baseline + index * lineHeight;
    let x = (width - lineWidth(line, space)) / 2;
    // No canvas equivalent of `text-decoration`, so the rule is drawn. Through
    // the middle of the lowercase body, which is where a struck word reads as
    // struck rather than as underlined.
    const rule = (from: number, width: number): void => {
      ctx.fillRect(
        from,
        y - Math.round(size * 0.28),
        width,
        Math.max(1, Math.round(size * 0.06)),
      );
    };

    line.forEach((word, index) => {
      for (const piece of word.pieces) {
        ctx.font = piece.font;
        ctx.fillText(piece.text, x, y);
        if (piece.strike) rule(x, piece.width);
        x += piece.width;
      }
      // Two struck words in a row are one struck phrase, so the space between
      // them is struck too — as it is on the page, where the browser does not
      // stop the rule at every word boundary.
      const next = line[index + 1];
      if (next && lastPiece(word)?.strike && next.pieces[0]?.strike) {
        rule(x, space);
      }
      x += space;
    });
  });
  ctx.restore();
}

/**
 * The runs as measured words.
 *
 * Every stretch of whitespace becomes one gap between words, newlines included:
 * a frame gets one centred sentence, and the note's own line breaks are not
 * part of it.
 */
function toWords(
  ctx: CanvasRenderingContext2D,
  runs: Run[],
  size: number,
): Word[] {
  const words: Word[] = [];
  let pieces: Piece[] = [];
  const end = (): void => {
    if (pieces.length === 0) return;
    words.push({
      pieces,
      width: pieces.reduce((total, piece) => total + piece.width, 0),
    });
    pieces = [];
  };

  for (const run of runs) {
    const font = runFont(run, size);
    run.text
      .replace(/\s+/g, " ")
      .split(" ")
      .forEach((part, index) => {
        // Not the first part of this run, so a space preceded it — and a space
        // is where one word ends and the next begins.
        if (index > 0) end();
        if (part) {
          pieces.push({
            text: part,
            font,
            strike: run.strike,
            width: measure(ctx, font, part),
          });
        }
      });
  }
  end();
  return words;
}

function wrap(words: Word[], space: number, maxWidth: number): Word[][] {
  const lines: Word[][] = [];
  let line: Word[] = [];
  let width = 0;
  for (const word of words) {
    if (line.length > 0 && width + space + word.width > maxWidth) {
      lines.push(line);
      line = [word];
      width = word.width;
      continue;
    }
    width += line.length > 0 ? space + word.width : word.width;
    line.push(word);
  }
  if (line.length > 0) lines.push(line);
  return lines;
}

const lastPiece = (word: Word): Piece | undefined =>
  word.pieces[word.pieces.length - 1];

const lineWidth = (line: Word[], space: number): number =>
  line.reduce((total, word) => total + word.width, 0) +
  space * Math.max(0, line.length - 1);

/** Shorten a line, in place, until it and a trailing ellipsis fit. */
function ellipsise(
  ctx: CanvasRenderingContext2D,
  line: Word[],
  base: string,
  space: number,
  maxWidth: number,
): void {
  const ellipsis = measure(ctx, base, "…");
  while (line.length > 0 && lineWidth(line, space) + ellipsis > maxWidth) {
    const word = line[line.length - 1];
    const piece = word.pieces[word.pieces.length - 1];
    const shorter = piece.text.slice(0, -1);
    if (shorter) {
      const width = measure(ctx, piece.font, shorter);
      word.width += width - piece.width;
      piece.text = shorter;
      piece.width = width;
      continue;
    }
    word.width -= piece.width;
    word.pieces.pop();
    if (word.pieces.length === 0) line.pop();
  }
  // Onto the end of the last word rather than as a word of its own, so there
  // is no space in front of it.
  const piece: Piece = { text: "…", font: base, strike: false, width: ellipsis };
  const last = line[line.length - 1];
  if (last) {
    last.pieces.push(piece);
    last.width += ellipsis;
  } else {
    line.push({ pieces: [piece], width: ellipsis });
  }
}

function measure(
  ctx: CanvasRenderingContext2D,
  font: string,
  text: string,
): number {
  ctx.font = font;
  return ctx.measureText(text).width;
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
  await loadFrameFonts();
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("This machine cannot provide a 2D canvas.");
  await drawFrame(ctx, canvas, project, entry);
  return canvas;
}
