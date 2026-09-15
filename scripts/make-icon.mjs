// Draws the Lapis app icon straight to a PNG: a lapis tile with a vein of gold
// running through it, the same blue and gold the app is themed in.
//
// Procedural rather than an SVG, so that the icon has no toolchain: rasterising
// an SVG needs a browser or a native library, and neither is a dependency worth
// carrying for a file that changes once a year.
//
//   node scripts/make-icon.mjs <out.png> [size]
//
// Then `npx tauri icon <out.png>` fans it out into `src-tauri/icons/`. Only the
// sizes a Windows build uses are kept; re-run it if another platform is added.

import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2] ?? "icon.png";
const SIZE = Number(process.argv[3] ?? 1024);

// Everything below is authored against a 1024 grid and scaled at the end, so
// the same numbers give any size.
const GRID = 1024;
const INSET = 64;
const RADIUS = 184;

const STONE_LIGHT = [0x2b, 0x52, 0xad];
const STONE_MID = [0x1a, 0x34, 0x79];
const STONE_DARK = [0x0e, 0x1b, 0x47];
const GOLD_DARK = [0xc8, 0x91, 0x3a];
const GOLD_MID = [0xe3, 0xb0, 0x4a];
const GOLD_LIGHT = [0xf4, 0xd6, 0x88];

/** Pyrite: the specks that tell you the blue is a stone and not a paint. */
const FLECKS = [
  [258, 318, 29],
  [712, 738, 37],
  [392, 860, 21],
  [826, 566, 23],
  [176, 470, 15],
  [614, 182, 18],
  [470, 690, 13],
];

const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
const lerp = (a, b, t) => a + (b - a) * t;
const mix = (a, b, t) => [
  lerp(a[0], b[0], t),
  lerp(a[1], b[1], t),
  lerp(a[2], b[2], t),
];
const smoothstep = (t) => {
  const x = clamp01(t);
  return x * x * (3 - 2 * x);
};

/** Coverage of a rounded rectangle, softened over one unit for the edge. */
function tileCoverage(x, y) {
  const half = (GRID - INSET * 2) / 2;
  const cx = GRID / 2;
  const cy = GRID / 2;
  const dx = Math.abs(x - cx) - (half - RADIUS);
  const dy = Math.abs(y - cy) - (half - RADIUS);
  const outside = Math.hypot(Math.max(dx, 0), Math.max(dy, 0));
  const distance =
    Math.min(Math.max(dx, dy), 0) + outside - RADIUS;
  return clamp01(0.5 - distance);
}

/** Where the vein runs, and how wide it is, at a given x. */
function veinAt(x) {
  const t = clamp01(x / GRID);
  return {
    centre: lerp(760, 330, smoothstep(t)),
    half: 58 + 14 * Math.sin(t * Math.PI),
    t,
  };
}

/** A soft radial blob, for the mottling that keeps the blue from going flat. */
function blob(x, y, cx, cy, rx, ry) {
  const d = Math.hypot((x - cx) / rx, (y - cy) / ry);
  return 1 - smoothstep(d);
}

function colourAt(x, y) {
  // The stone: a diagonal gradient, then mottled light and dark.
  const diagonal = clamp01((x / GRID + y / GRID) / 2);
  let rgb =
    diagonal < 0.55
      ? mix(STONE_LIGHT, STONE_MID, diagonal / 0.55)
      : mix(STONE_MID, STONE_DARK, (diagonal - 0.55) / 0.45);

  rgb = mix(rgb, [0x4a, 0x75, 0xd6], 0.55 * blob(x, y, 300, 260, 330, 280));
  rgb = mix(rgb, [0x0a, 0x14, 0x40], 0.7 * blob(x, y, 800, 820, 300, 260));
  rgb = mix(rgb, [0x0a, 0x14, 0x40], 0.6 * blob(x, y, 890, 170, 230, 210));

  // The vein, lit from the far end so it reads as metal rather than as paint.
  const vein = veinAt(x);
  const gold =
    vein.t < 0.45
      ? mix(GOLD_DARK, GOLD_MID, vein.t / 0.45)
      : mix(GOLD_MID, GOLD_LIGHT, (vein.t - 0.45) / 0.55);
  const edge = Math.abs(y - vein.centre) - vein.half;
  rgb = mix(rgb, gold, 1 - smoothstep(edge + 0.5));

  for (const [fx, fy, r] of FLECKS) {
    const d = Math.hypot(x - fx, y - fy) - r;
    rgb = mix(rgb, GOLD_MID, 0.95 * (1 - smoothstep(d + 0.5)));
  }

  return rgb;
}

// --- render ----------------------------------------------------------------

const scale = GRID / SIZE;
const pixels = Buffer.alloc(SIZE * SIZE * 4);

for (let py = 0; py < SIZE; py += 1) {
  for (let px = 0; px < SIZE; px += 1) {
    // 2x2 supersampling: the tile's corners and the flecks are curves, and one
    // sample per pixel leaves them visibly stepped at the small sizes.
    let r = 0;
    let g = 0;
    let b = 0;
    let a = 0;
    for (const oy of [0.25, 0.75]) {
      for (const ox of [0.25, 0.75]) {
        const x = (px + ox) * scale;
        const y = (py + oy) * scale;
        const cover = tileCoverage(x, y);
        if (cover <= 0) continue;
        const [cr, cg, cb] = colourAt(x, y);
        r += cr * cover;
        g += cg * cover;
        b += cb * cover;
        a += cover;
      }
    }
    const at = (py * SIZE + px) * 4;
    if (a > 0) {
      // Un-premultiply: the colour is the average of the covered samples, and
      // the alpha is how much of the pixel they covered.
      pixels[at] = Math.round(r / a);
      pixels[at + 1] = Math.round(g / a);
      pixels[at + 2] = Math.round(b / a);
      pixels[at + 3] = Math.round((a / 4) * 255);
    }
  }
}

// --- PNG -------------------------------------------------------------------

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buf) {
  let c = -1;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
// 10, 11, 12: deflate, adaptive filtering, no interlace — all zero.

// Every scanline gets filter byte 0: the image is smooth gradients, which
// deflate handles well enough that choosing filters per line is not worth it.
const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y += 1) {
  pixels.copy(raw, y * (SIZE * 4 + 1) + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
}

writeFileSync(
  OUT,
  Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]),
);

console.log(`${OUT} — ${SIZE}x${SIZE}`);
