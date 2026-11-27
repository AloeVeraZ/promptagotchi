// Generates pets/volt/pet.json — Volt, a neon cyberpunk critter (an original design).
// Dark chrome body, black-glass visor with glowing cyan eyes, hot-pink mohawk, one chrome
// cyber-arm, pink-soled boots. Types on a floating hologram while Claude works.
//
//   node scripts/build-volt.mjs
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const W = 32, H = 28;

const palette = {
  '.': 'transparent',
  k: '#1C1830', // outline / boots
  b: '#3B335A', // body
  h: '#5A4E88', // body highlight
  v: '#0A0D18', // visor glass
  c: '#2EF2FF', // neon cyan (eyes, code)
  C: '#1C8C9E', // dim cyan
  p: '#FF2E88', // hot pink (mohawk, soles)
  P: '#A8175A', // dark pink
  y: '#F5F03C', // neon yellow (chest light, sparks)
  m: '#CBD3E2', // chrome
  M: '#8790A6', // chrome shade
  r: '#FF3B4E', // error red
  a: '#FFB23B', // amber (needs you)
  s: 'rgba(0,0,0,0.22)', // ground shadow
  g: 'rgba(46,242,255,0.32)', // hologram over nothing
  G: 'rgba(46,242,255,0.7)',  // hologram edge over nothing
  q: '#367692', // hologram over the body (pre-mixed)
  Q: '#32B9CD', // hologram edge over the body
};
const BODY = new Set(['k', 'b', 'h', 'v', 'y', 'm', 'M', 'p', 'P', 'C', 'a', 'r']);

const blank = () => Array.from({ length: H }, () => Array(W).fill('.'));
const px = (g, x, y, c) => { if (x >= 0 && x < W && y >= 0 && y < H) g[y][x] = c; };
const get = (g, x, y) => (x >= 0 && x < W && y >= 0 && y < H ? g[y][x] : '.');
const rect = (g, x0, y0, x1, y1, c) => { for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) px(g, x, y, c); };
const rows = (g) => g.map((r) => r.join(''));

function shadow(g, ground, lift = 0) {
  const s = Math.min(4, Math.floor(lift / 1.5));
  rect(g, 9 + s, ground, 22 - s, ground, 's');
}

function legs(g, bottom, kind = 'stand') {
  const L = [11, 18];
  if (kind === 'stand') for (const x of L) { rect(g, x, bottom + 1, x + 2, bottom + 2, 'k'); rect(g, x, bottom + 3, x + 2, bottom + 3, 'p'); }
  if (kind === 'tuck') for (const x of L) { rect(g, x, bottom + 1, x + 2, bottom + 1, 'k'); rect(g, x, bottom + 2, x + 2, bottom + 2, 'p'); }
  if (kind === 'dangleA' || kind === 'dangleB') {
    const sw = kind === 'dangleA' ? 0 : 1;
    rect(g, 10 - sw, bottom + 1, 12 - sw, bottom + 3, 'k'); rect(g, 10 - sw, bottom + 4, 12 - sw, bottom + 4, 'p');
    rect(g, 19 + sw, bottom + 1, 21 + sw, bottom + 3, 'k'); rect(g, 19 + sw, bottom + 4, 21 + sw, bottom + 4, 'p');
  }
}

function mohawk(g, top, sway = 0) {
  rect(g, 13, top - 1, 18, top - 1, 'P');
  rect(g, 13 + sway, top - 2, 14 + sway, top - 2, 'p');
  rect(g, 16 + sway, top - 2, 17 + sway, top - 2, 'p');
  px(g, 14 + sway, top - 3, 'p');
  px(g, 17 + sway, top - 3, 'p');
  px(g, 18 + sway, top - 4, 'p');
}

/** Eyes live inside the visor (3 rows starting at vy). */
function eyes(g, vy, kind = 'open', look = 0) {
  const L = 12 + look, R = 18 + look;
  for (const x of [L, R]) {
    switch (kind) {
      case 'open': rect(g, x, vy + 1, x + 1, vy + 1, 'c'); px(g, x - 1, vy + 1, 'C'); px(g, x + 2, vy + 1, 'C'); break;
      case 'down': rect(g, x, vy + 2, x + 1, vy + 2, 'c'); break;
      case 'blink': rect(g, x, vy + 1, x + 1, vy + 1, 'C'); break;
      case 'alert': rect(g, x, vy, x + 1, vy + 2, 'a'); break;
      case 'happy': px(g, x - 1, vy + 2, 'c'); rect(g, x, vy + 1, x + 1, vy + 1, 'c'); px(g, x + 2, vy + 2, 'c'); break;
      case 'closed': rect(g, x - 1, vy + 1, x + 2, vy + 1, 'C'); break;
      case 'x': px(g, x - 1, vy, 'r'); px(g, x + 1, vy, 'r'); px(g, x, vy + 1, 'r'); px(g, x - 1, vy + 2, 'r'); px(g, x + 1, vy + 2, 'r'); break;
      default: break;
    }
  }
}

// side -1 = left (normal arm), +1 = right (chrome cyber-arm).
function arm(g, side, top, pose = 'nub') {
  const inner = side < 0 ? 8 : 23;
  const o = (n) => inner + side * n;
  const c = side < 0 ? 'b' : 'm';
  const tip = side < 0 ? 'h' : 'M';
  const r = (a, y0, b, y1) => rect(g, Math.min(o(a), o(b)), y0, Math.max(o(a), o(b)), y1, c);
  switch (pose) {
    case 'nub': r(0, top + 6, 2, top + 7); px(g, o(2), top + 7, tip); break;
    case 'down': r(0, top + 7, 1, top + 9); px(g, o(1), top + 9, tip); break;
    case 'up': r(0, top, 1, top + 7); px(g, o(1), top, tip); break;
    case 'wave': r(0, top + 4, 1, top + 7); r(1, top, 2, top + 4); px(g, o(2), top, tip); break;
    case 'upout': r(0, top + 5, 1, top + 7); r(1, top + 2, 2, top + 5); r(2, top - 1, 3, top + 2); px(g, o(3), top - 1, tip); break;
    case 'out': r(0, top + 6, 3, top + 7); px(g, o(3), top + 6, tip); break;
    default: break;
  }
}

function volt(g, { dy = 0, squash = 0, eye = 'open', look = 0, armL = 'nub', armR = 'nub', leg = 'stand', lift = 0, sway = 0, visor = 'v' } = {}) {
  const bottom = 20 + dy;
  const top = 10 + dy + squash;
  shadow(g, 24, lift || -dy);
  legs(g, bottom, leg);
  rect(g, 9, top, 22, bottom, 'b');
  rect(g, 10, top, 21, top, 'h');
  px(g, 9, top, '.');
  px(g, 22, top, '.');
  rect(g, 9, bottom, 22, bottom, 'k');
  rect(g, 22, top + 1, 22, bottom, 'k');
  mohawk(g, top, sway);
  const vy = top + 3;
  rect(g, 9, vy, 22, vy + 2, visor);
  eyes(g, vy, eye, look);
  rect(g, 15, bottom - 2, 16, bottom - 2, 'y');
  arm(g, -1, top, armL);
  arm(g, +1, top, armR);
  return g;
}

const frame = (o) => rows(volt(blank(), o));

// ---------------------------------------------------------------- working
// A floating hologram terminal: code scrolls on it while Volt types on a holo keyboard.
const CODE = [
  [[0, 5], [7, 9]], [[2, 7]], [[0, 3], [5, 10]], [[2, 4], [6, 8]], [[0, 8]], [[2, 5], [7, 10]],
];
function holo(g, x0, y0, x1, y1, edge) {
  for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) {
    const isEdge = edge && (y === y0 || y === y1 || x === x0 || x === x1);
    const over = BODY.has(get(g, x, y));
    px(g, x, y, isEdge ? (over ? 'Q' : 'G') : over ? 'q' : 'g');
  }
}
function workFrame(i, leftUp, rightUp, spark) {
  const g = blank();
  volt(g, { eye: 'down', armL: 'none', armR: 'none' });
  // arms reaching forward to the keys
  rect(g, 7, 17, 8, leftUp ? 20 : 21, 'b'); px(g, 7, leftUp ? 20 : 21, 'h');
  rect(g, 23, 17, 24, rightUp ? 20 : 21, 'm'); px(g, 24, rightUp ? 20 : 21, 'M');
  // the hologram screen with two scrolling lines of code
  holo(g, 10, 15, 21, 20, true);
  const a = CODE[i % CODE.length], b = CODE[(i + 2) % CODE.length];
  for (const [s, e] of a) rect(g, 11 + s, 17, 11 + Math.min(e, 9), 17, 'c');
  for (const [s, e] of b) rect(g, 11 + s, 19, 11 + Math.min(e, 9), 19, 'C');
  // holo keyboard
  holo(g, 6, 22, 25, 22, false);
  for (let x = 7; x <= 24; x += 3) px(g, x, 22, get(g, x, 22) === 'q' ? 'Q' : 'G');
  if (spark === 'L') { px(g, 6, 20, 'y'); px(g, 5, 19, 'y'); }
  if (spark === 'R') { px(g, 25, 20, 'y'); px(g, 26, 19, 'y'); }
  return rows(g);
}

const animations = {
  idle: {
    durations: [900, 500, 900, 500, 260, 110, 1100, 500],
    frames: [
      frame({}),
      frame({ squash: 1 }),
      frame({ sway: 1 }),
      frame({ squash: 1 }),
      frame({}),
      frame({ eye: 'blink' }),
      frame({}),
      frame({ squash: 1, sway: 1 }),
    ],
  },
  working: {
    durations: [110, 110, 110, 110, 110, 110, 110, 160],
    frames: [
      workFrame(0, false, true),
      workFrame(1, true, false, 'L'),
      workFrame(2, false, true, 'R'),
      workFrame(3, true, false),
      workFrame(4, false, true),
      workFrame(5, true, false, 'L'),
      workFrame(6, false, false),
      workFrame(7, true, true),
    ],
  },
  question: {
    durations: [240, 200, 240, 200],
    frames: [
      frame({ eye: 'alert', armR: 'up' }),
      frame({ eye: 'alert', armR: 'wave', dy: -1, lift: 1, sway: 1 }),
      frame({ eye: 'open', armR: 'up' }),
      frame({ eye: 'alert', armR: 'upout', dy: -1, lift: 1 }),
    ],
  },
  done: {
    durations: [130, 120, 160, 120, 140, 900],
    frames: [
      frame({ eye: 'happy', squash: 1, armL: 'out', armR: 'out' }),
      frame({ eye: 'happy', dy: -3, armL: 'up', armR: 'up', leg: 'tuck', sway: 1 }),
      frame({ eye: 'happy', dy: -5, armL: 'upout', armR: 'upout', leg: 'tuck', sway: -1 }),
      frame({ eye: 'happy', dy: -2, armL: 'up', armR: 'up' }),
      frame({ eye: 'happy', squash: 1, armL: 'out', armR: 'out' }),
      frame({ eye: 'happy' }),
    ],
  },
  error: {
    durations: [420, 90, 420, 90],
    frames: [
      frame({ eye: 'x', squash: 1, armL: 'down', armR: 'down' }),
      frame({ eye: 'x', squash: 1, armL: 'down', armR: 'down', look: 1, sway: 1 }),
      frame({ eye: 'x', squash: 2, armL: 'down', armR: 'down' }),
      frame({ eye: 'blink', squash: 2, armL: 'down', armR: 'down', look: -1 }),
    ],
  },
  sleep: {
    durations: [1100, 1100],
    frames: [
      frame({ eye: 'closed', squash: 2, armL: 'down', armR: 'down', leg: 'tuck' }),
      frame({ eye: 'closed', squash: 1, armL: 'down', armR: 'down', leg: 'tuck' }),
    ],
  },
  drag: {
    durations: [160, 160],
    frames: [
      frame({ eye: 'alert', dy: -3, armL: 'up', armR: 'up', leg: 'dangleA', lift: 6, sway: 1 }),
      frame({ eye: 'alert', dy: -3, armL: 'up', armR: 'up', leg: 'dangleB', lift: 6, sway: -1 }),
    ],
  },
  wave: {
    loop: false,
    durations: [140, 140, 140, 140, 140, 220],
    frames: [
      frame({ eye: 'happy', armR: 'up' }),
      frame({ eye: 'happy', armR: 'wave' }),
      frame({ eye: 'happy', armR: 'up' }),
      frame({ eye: 'happy', armR: 'wave' }),
      frame({ eye: 'happy', armR: 'up' }),
      frame({}),
    ],
  },
  // Idle fidgets: a visor scan, and a stretch.
  look: {
    loop: false,
    durations: [160, 160, 160, 300, 160, 160, 160, 300],
    frames: [-1, -2, -3, -3, -1, 1, 2, 0].map((look) => frame({ look })),
  },
  stretch: {
    loop: false,
    durations: [160, 160, 650, 160, 200],
    frames: [
      frame({ squash: 1, armL: 'out', armR: 'out' }),
      frame({ squash: -1, eye: 'closed', armL: 'up', armR: 'up' }),
      frame({ squash: -2, eye: 'closed', armL: 'upout', armR: 'upout', sway: 1 }),
      frame({ squash: -1, eye: 'closed', armL: 'up', armR: 'up' }),
      frame({ squash: 1, eye: 'blink' }),
    ],
  },
  // Whipped! A flinch: red X visor, a hop, then on alert (then Hustle mode kicks in).
  hit: {
    loop: false,
    durations: [110, 120, 140, 160, 260],
    frames: [
      frame({ eye: 'x', squash: 2, armL: 'out', armR: 'out' }),
      frame({ eye: 'x', dy: -3, armL: 'up', armR: 'up', leg: 'tuck', sway: 1 }),
      frame({ eye: 'alert', dy: -2, armL: 'upout', armR: 'upout', leg: 'tuck', sway: -1 }),
      frame({ eye: 'alert', squash: 1, armL: 'out', armR: 'out' }),
      frame({ eye: 'alert' }),
    ],
  },
  jump: {
    loop: false,
    durations: [110, 110, 140, 120, 110, 160],
    frames: [
      frame({ eye: 'alert', squash: 2, armL: 'out', armR: 'out' }),
      frame({ eye: 'alert', dy: -4, armL: 'up', armR: 'up', leg: 'tuck', sway: 1 }),
      frame({ eye: 'alert', dy: -6, armL: 'upout', armR: 'upout', leg: 'tuck', sway: -1 }),
      frame({ eye: 'alert', dy: -4, armL: 'up', armR: 'up' }),
      frame({ eye: 'alert', squash: 2, armL: 'out', armR: 'out' }),
      frame({ eye: 'alert', armR: 'up' }),
    ],
  },
};

// Arms drawn by hand in the working frames: 'none' poses draw nothing.
const pet = {
  id: 'volt',
  name: 'Volt',
  author: 'Promptagotchi',
  description: 'A neon cyberpunk critter with a glowing visor, a pink mohawk and a chrome cyber-arm. Hacks on a hologram while Claude works.',
  frameWidth: W,
  frameHeight: H,
  pixelArt: true,
  palette,
  animations,
};

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.join(here, '..', 'pets', 'volt', 'pet.json');
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, JSON.stringify(pet, null, 2) + '\n');
console.log('wrote', path.relative(process.cwd(), out));
