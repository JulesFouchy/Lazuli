// Draws the repository banner: the same artwork as the icon, run out wide.
//
//   node scripts/make-banner.mjs <out.png> [width] [height]
//
// Full-bleed rather than a tile — a banner has no corners to round — and with
// more flecks, because seven of them spread over this much stone reads as a
// blank field with a few specks in the corners.
//
// The committed one is `assets/banner.png`, at the size GitHub uses for a
// social preview, which is also wide enough to sit at the top of the README:
//
//   node scripts/make-banner.mjs assets/banner.png 1280 640

import { paintAt, veinAt, writePng } from "./lapis-art.mjs";

const OUT = process.argv[2] ?? "banner.png";
const WIDTH = Number(process.argv[3] ?? 1280);
const HEIGHT = Number(process.argv[4] ?? 640);

/**
 * Pyrite, as `[u, offset from the vein, radius]` in clusters along it.
 *
 * Placed against the vein rather than against the canvas because that is where
 * it sits in the stone, and because an even scatter across this much blue reads
 * as polka dots — the regularity is invisible in an icon holding seven of them
 * and obvious the moment there is room for twenty.
 *
 * Radii are fractions of the *height*, the shorter side, which is what
 * `paintAt` measures them against.
 */
const FLECK_SPECS = [
  [0.055, -0.135, 0.0125],
  [0.082, -0.196, 0.0085],
  [0.119, 0.118, 0.0142],
  [0.148, -0.108, 0.0068],
  [0.215, 0.238, 0.0074],
  [0.281, 0.132, 0.0108],
  [0.312, 0.201, 0.0158],
  [0.344, -0.124, 0.0092],
  [0.366, -0.223, 0.0061],
  [0.418, 0.284, 0.0058],
  [0.47, -0.268, 0.0069],
  [0.524, -0.131, 0.0146],
  [0.556, -0.215, 0.0101],
  [0.589, 0.126, 0.0079],
  [0.652, 0.335, 0.0066],
  [0.742, 0.147, 0.0173],
  [0.771, 0.238, 0.0094],
  [0.804, -0.119, 0.0112],
  [0.906, 0.163, 0.0129],
  [0.938, 0.262, 0.0083],
  [0.957, -0.126, 0.0105],
];

const FLECKS = FLECK_SPECS.map(([u, offset, radius]) => [
  u,
  veinAt(u).centre + offset,
  radius,
]);

// One pixel measured on the shorter side, which is what the edges are soft over.
const softness = 1 / HEIGHT;
const spread = WIDTH / HEIGHT;

writePng(OUT, WIDTH, HEIGHT, (u, v) => {
  const [r, g, b] = paintAt(u, v, { flecks: FLECKS, spread, softness });
  return [r, g, b, 1];
});
