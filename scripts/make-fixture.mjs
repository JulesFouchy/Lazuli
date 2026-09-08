// Generates a throwaway Journaley project to develop and test against.
//
//   node scripts/make-fixture.mjs [folder] [entryCount]
//
// Entries are spread over a realistic-looking span with gaps and the odd
// double day, and each gets a generated image so the timeline has something to
// show. Used both for everyday poking and for the 1500-entry scale check.
//
// Defaults into the OS temp folder. Fixtures are disposable noise and do not
// belong next to real projects, so opting into a location is deliberate.

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { randomUUID } from "node:crypto";

const OUT = process.argv[2] ?? join(tmpdir(), "journaley-fixture");
const COUNT = Number(process.argv[3] ?? 24);

// --- a tiny PNG encoder, so the fixture needs no image dependencies --------

function crc32(buf) {
  let c = ~0;
  for (const byte of buf) {
    c ^= byte;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

/** A soft two-tone gradient with a bit of noise, so cards look distinct. */
function gradientPng(width, height, seed) {
  const rand = mulberry32(seed);
  const hueA = rand() * 360;
  const hueB = (hueA + 40 + rand() * 120) % 360;
  const [r1, g1, b1] = hslToRgb(hueA, 0.45, 0.35);
  const [r2, g2, b2] = hslToRgb(hueB, 0.5, 0.6);

  const raw = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    const rowStart = y * (width * 3 + 1);
    raw[rowStart] = 0; // filter: none
    for (let x = 0; x < width; x++) {
      const t = (x / width) * 0.45 + (y / height) * 0.55;
      const n = (rand() - 0.5) * 10;
      const i = rowStart + 1 + x * 3;
      raw[i] = clamp(r1 + (r2 - r1) * t + n);
      raw[i + 1] = clamp(g1 + (g2 - g1) * t + n);
      raw[i + 2] = clamp(b1 + (b2 - b1) * t + n);
    }
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 2; // truecolour
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 6 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const clamp = (v) => Math.max(0, Math.min(255, Math.round(v)));

function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function hslToRgb(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] =
    h < 60 ? [c, x, 0]
    : h < 120 ? [x, c, 0]
    : h < 180 ? [0, c, x]
    : h < 240 ? [0, x, c]
    : h < 300 ? [x, 0, c]
    : [c, 0, x];
  return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}

// --- the project ------------------------------------------------------------

const NOTES = [
  "Finally got the dovetails to close without a gap.",
  "Squared up the legs. Took three attempts and a lot of swearing.",
  "Glue-up day. Clamps everywhere.",
  "Sanded to 220. My arms hurt.",
  "First coat of oil. The grain came alive.",
  "Cut the mortises by hand for once.",
  "Realised the top was out of flat and started again.",
  "Short session, just sharpening.",
  "Fitted the drawer. It sticks slightly in the humidity.",
  "Cleaned the shop instead of working. Counts as progress.",
  "Test assembly. It stands up on its own.",
  "Added the shelf underneath.",
  "Fixed yesterday's mistake, mostly.",
  "Nothing worked today but I learned something.",
  "Done. Moved it inside.",
];

rmSync(OUT, { recursive: true, force: true });
mkdirSync(join(OUT, "entries"), { recursive: true });
mkdirSync(join(OUT, "cover"), { recursive: true });

const rand = mulberry32(20260908);

// Two cover attempts, one chosen, to exercise the "keep every attempt" rule.
writeFileSync(join(OUT, "cover", "bench-wide.png"), gradientPng(1600, 500, 1));
writeFileSync(join(OUT, "cover", "bench-wide-take2.png"), gradientPng(1600, 500, 2));

// Walk backwards from today so the newest entry is always recent.
const dates = [];
let cursor = new Date();
for (let i = 0; i < COUNT; i++) {
  dates.push(new Date(cursor));
  // Mostly short gaps, occasionally a long pause, so the timeline has both.
  const roll = rand();
  const gap = roll < 0.12 ? 0 : roll < 0.6 ? 1 : roll < 0.9 ? 2 + Math.floor(rand() * 5) : 10 + Math.floor(rand() * 50);
  cursor = new Date(cursor.getTime() - gap * 86_400_000);
}
dates.reverse();

const pad = (n) => String(n).padStart(2, "0");

/** RFC 3339 with the machine's real offset: the offset is load-bearing. */
function rfc3339(date, hour, minute) {
  const local = new Date(date);
  local.setHours(hour, minute, 0, 0);
  const offset = -local.getTimezoneOffset();
  const sign = offset < 0 ? "-" : "+";
  const abs = Math.abs(offset);
  return (
    `${local.getFullYear()}-${pad(local.getMonth() + 1)}-${pad(local.getDate())}` +
    `T${pad(local.getHours())}:${pad(local.getMinutes())}:00` +
    `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
  );
}

let startDate = null;

dates.forEach((date, index) => {
  const id = randomUUID();
  const dir = join(OUT, "entries", id);
  mkdirSync(dir, { recursive: true });

  // Every eighth entry is written in the small hours, to exercise the 5am
  // rule: these file under the *previous* day.
  const lateNight = index % 8 === 3;
  const hour = lateNight ? 1 : 9 + Math.floor(rand() * 12);
  const minute = Math.floor(rand() * 60);
  const created = rfc3339(date, hour, minute);

  if (startDate === null) {
    const first = new Date(date);
    first.setHours(hour, minute, 0, 0);
    // The project starts on the first entry's journal day: shift back 5h.
    const shifted = new Date(first.getTime() - 5 * 3600_000);
    startDate = `${shifted.getFullYear()}-${pad(shifted.getMonth() + 1)}-${pad(shifted.getDate())}`;
  }

  // Some entries keep several attempts; one in ten has no image at all.
  const attempts = rand() < 0.25 ? 3 : rand() < 0.5 ? 2 : 1;
  const names = [];
  for (let a = 0; a < attempts; a++) {
    const name = a === 0 ? "shot.png" : `shot-take${a + 1}.png`;
    const portrait = rand() < 0.3;
    writeFileSync(
      join(dir, name),
      gradientPng(portrait ? 720 : 1280, portrait ? 1000 : 720, index * 7 + a),
    );
    names.push(name);
  }

  const chosen = rand() < 0.1 ? null : names[names.length - 1];
  const note = NOTES[index % NOTES.length];

  writeFileSync(
    join(dir, "entry.md"),
    `---\ncreated: ${created}\nimage: ${chosen ?? "null"}\n---\n\n${note}\n`,
  );
});

writeFileSync(
  join(OUT, "journaley.yaml"),
  `name: Woodworking bench\nstart_date: ${startDate}\ncover: bench-wide-take2.png\n`,
);

console.log(`wrote ${COUNT} entries to ${OUT} (start_date ${startDate})`);
