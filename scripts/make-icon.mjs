// Draws the Lapis app icon: the artwork in `lapis-art.mjs`, cut to a rounded
// tile. The vein is the whole of what survives at 16px, which is why it is as
// wide as it is.
//
//   node scripts/make-icon.mjs <out.png> [size]
//
// Then `npx tauri icon <out.png>` fans it out into `src-tauri/icons/`. That
// tool does not write `256x256.png`, so render that size over it by hand.
// Only the sizes a Windows build uses are kept; re-run it if another platform
// is added.

import { clamp01, paintAt, writePng } from "./lapis-art.mjs";

const OUT = process.argv[2] ?? "icon.png";
const SIZE = Number(process.argv[3] ?? 1024);

/** The corner radius and the margin, as fractions of the tile. */
const INSET = 64 / 1024;
const RADIUS = 184 / 1024;

/**
 * Pyrite: the specks that tell you the blue is a stone and not a paint.
 *
 * Over 1024, the grid they were placed on, so that changing the renderer
 * cannot move them by a fraction of a pixel and rewrite every icon file.
 */
const FLECKS = [
  [258, 318, 29],
  [712, 738, 37],
  [392, 860, 21],
  [826, 566, 23],
  [176, 470, 15],
  [614, 182, 18],
  [470, 690, 13],
].map(([x, y, r]) => [x / 1024, y / 1024, r / 1024]);

const softness = 1 / SIZE;

/** Coverage of the rounded tile, softened over one pixel at the edge. */
function tileCoverage(u, v) {
  const half = 0.5 - INSET;
  const du = Math.abs(u - 0.5) - (half - RADIUS);
  const dv = Math.abs(v - 0.5) - (half - RADIUS);
  const outside = Math.hypot(Math.max(du, 0), Math.max(dv, 0));
  const distance = Math.min(Math.max(du, dv), 0) + outside - RADIUS;
  return clamp01(0.5 - distance / softness);
}

writePng(OUT, SIZE, SIZE, (u, v) => {
  const cover = tileCoverage(u, v);
  if (cover <= 0) return [0, 0, 0, 0];
  const [r, g, b] = paintAt(u, v, { flecks: FLECKS, softness });
  return [r, g, b, cover];
});
