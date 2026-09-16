// The Lazuli artwork — a lazuli ground with a vein of gold running through it —
// and a PNG writer to put it on disk.
//
// Everything is authored in normalised coordinates, `u` and `v` both running 0
// to 1 across whatever is being drawn. That is what lets the icon and the
// banner be the same picture at two aspect ratios rather than two drawings
// that have to be kept looking alike by hand.
//
// Procedural rather than an SVG, so that none of this needs a toolchain:
// rasterising an SVG needs a browser or a native library, and neither is a
// dependency worth carrying for files that change once a year.

import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

export const STONE_LIGHT = [0x2b, 0x52, 0xad];
export const STONE_MID = [0x1a, 0x34, 0x79];
export const STONE_DARK = [0x0e, 0x1b, 0x47];
export const STONE_SHADOW = [0x0a, 0x14, 0x40];
export const STONE_SHEEN = [0x4a, 0x75, 0xd6];

export const GOLD_DARK = [0xc8, 0x91, 0x3a];
export const GOLD_MID = [0xe3, 0xb0, 0x4a];
export const GOLD_LIGHT = [0xf4, 0xd6, 0x88];

export const clamp01 = (value) => (value < 0 ? 0 : value > 1 ? 1 : value);
export const lerp = (a, b, t) => a + (b - a) * t;
export const mix = (a, b, t) => [
  lerp(a[0], b[0], t),
  lerp(a[1], b[1], t),
  lerp(a[2], b[2], t),
];

export function smoothstep(t) {
  const x = clamp01(t);
  return x * x * (3 - 2 * x);
}

/** Every channel times the same factor, which is what holds the hue. */
export const scale = (rgb, factor) => [
  rgb[0] * factor,
  rgb[1] * factor,
  rgb[2] * factor,
];

/** A soft elliptical blob, for the mottling that keeps the blue from going flat. */
export function blob(u, v, cu, cv, ru, rv) {
  return 1 - smoothstep(Math.hypot((u - cu) / ru, (v - cv) / rv));
}

/**
 * Where the vein runs, and how thick it is, at a given `u`.
 *
 * The same curve at every aspect ratio, so widening the canvas lays the vein
 * down shallower instead of cropping it.
 */
export function veinAt(u) {
  const t = clamp01(u);
  return {
    t,
    // Written over 1024 because that is the grid the shape was drawn on, and
    // the exact fractions keep the icon identical to the pixel.
    centre: lerp(760 / 1024, 330 / 1024, smoothstep(t)),
    half: (58 + 14 * Math.sin(t * Math.PI)) / 1024,
  };
}

/**
 * The ground, before the vein and the flecks are laid over it.
 *
 * `spread` stretches the mottling with the canvas: on a wide one the blobs
 * would otherwise be squeezed into ellipses tall enough to read as bands.
 */
export function stoneAt(u, v, spread = 1) {
  const diagonal = clamp01((u + v) / 2);
  let rgb =
    diagonal < 0.55
      ? mix(STONE_LIGHT, STONE_MID, diagonal / 0.55)
      : mix(STONE_MID, STONE_DARK, (diagonal - 0.55) / 0.45);

  // Over 1024, the grid this was drawn on: exact fractions keep the icon
  // identical to the pixel when anything here is refactored.
  const g = 1 / 1024;
  rgb = mix(
    rgb,
    STONE_SHEEN,
    0.55 * blob(u, v, 300 * g, 260 * g, 330 * g * spread, 280 * g),
  );
  rgb = mix(
    rgb,
    STONE_SHADOW,
    0.7 * blob(u, v, 800 * g, 820 * g, 300 * g * spread, 260 * g),
  );
  rgb = mix(
    rgb,
    STONE_SHADOW,
    0.6 * blob(u, v, 890 * g, 170 * g, 230 * g * spread, 210 * g),
  );
  return rgb;
}

/** The vein's colour at `t` along it, lit from the far end so it reads as metal. */
export function goldAt(t) {
  return t < 0.45
    ? mix(GOLD_DARK, GOLD_MID, t / 0.45)
    : mix(GOLD_MID, GOLD_LIGHT, (t - 0.45) / 0.55);
}

/**
 * The whole picture at one point.
 *
 * `softness` is how wide an edge is in normalised units — one pixel, passed in
 * by the renderer, since the same fraction is a different number of pixels at
 * every size.
 */
export function paintAt(u, v, { flecks, spread = 1, softness }) {
  let rgb = stoneAt(u, v, spread);

  const vein = veinAt(u);
  const edge = Math.abs(v - vein.centre) - vein.half;
  rgb = mix(rgb, goldAt(vein.t), 1 - smoothstep(edge / softness + 0.5));

  for (const [fu, fv, fr] of flecks) {
    // Measured against the shorter side, so a fleck stays a circle.
    const distance = Math.hypot((u - fu) * spread, v - fv) - fr;
    rgb = mix(rgb, GOLD_MID, 0.95 * (1 - smoothstep(distance / softness + 0.5)));
  }

  return rgb;
}

/**
 * Render and write a PNG.
 *
 * `sample(u, v)` returns `[r, g, b, coverage]`. 2x2 supersampled: the curves
 * are all analytic, and one sample per pixel leaves them visibly stepped at the
 * small icon sizes.
 */
export function writePng(path, width, height, sample) {
  const pixels = Buffer.alloc(width * height * 4);

  for (let py = 0; py < height; py += 1) {
    for (let px = 0; px < width; px += 1) {
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      for (const oy of [0.25, 0.75]) {
        for (const ox of [0.25, 0.75]) {
          const [sr, sg, sb, cover] = sample(
            (px + ox) / width,
            (py + oy) / height,
          );
          if (cover <= 0) continue;
          r += sr * cover;
          g += sg * cover;
          b += sb * cover;
          a += cover;
        }
      }
      const at = (py * width + px) * 4;
      if (a > 0) {
        // Un-premultiply: the colour is the average of the covered samples,
        // and the alpha is how much of the pixel they covered.
        pixels[at] = Math.round(r / a);
        pixels[at + 1] = Math.round(g / a);
        pixels[at + 2] = Math.round(b / a);
        pixels[at + 3] = Math.round((a / 4) * 255);
      }
    }
  }

  writeFileSync(path, encodePng(pixels, width, height));
  console.log(`${path} — ${width}x${height}`);
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

function encodePng(pixels, width, height) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  // 10, 11, 12: deflate, adaptive filtering, no interlace — all zero.

  // Every scanline gets filter byte 0: the image is smooth gradients, which
  // deflate handles well enough that choosing filters per line is not worth it.
  const stride = width * 4;
  const raw = Buffer.alloc(height * (stride + 1));
  for (let y = 0; y < height; y += 1) {
    pixels.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}
