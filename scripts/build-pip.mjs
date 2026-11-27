// Generates pets/pip/pet.json — Pip, the default Promptagotchi pet.
// Pip is drawn with code (rects + pixels) so the art stays consistent across states,
// but the output is a plain text-pixel pet.json that anyone can open and hand-edit.
//
//   node scripts/build-pip.mjs
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const W = 32, H = 28, GROUND = 23;

const palette = {
  '.': 'transparent',
  o: '#D97757', // body
  h: '#EC9A79', // top highlight
  d: '#B3573A', // shade
  e: '#2A1914', // eyes
  w: '#FFFFFF', // eye glint
  b: '#F7A3A0', // blush
  s: 'rgba(0,0,0,0.22)', // ground shadow
  L: '#C3CAD2', // laptop lid
  l: '#8E97A3', // lid edge
  k: '#E3E7EB', // laptop deck
  K: '#9AA3AD', // deck shade
  g: '#FFD27A', // glowing logo / key sparkle
};

const blank = () => Array.from({ length: H }, () => Array(W).fill('.'));
const px = (g, x, y, c) => { if (x >= 0 && x < W && y >= 0 && y < H) g[y][x] = c; };
const rect = (g, x0, y0, x1, y1, c) => { for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) px(g, x, y, c); };
const rows = (g) => g.map((r) => r.join(''));

function shadow(g, lift = 0) {
  const shrink = Math.min(4, Math.floor(lift / 1.5));
  rect(g, 9 + shrink, GROUND, 22 - shrink, GROUND, 's');
}

function legs(g, bottom, kind = 'stand') {
  const xs = [9, 12, 18, 21];
  if (kind === 'stand') for (const x of xs) rect(g, x, bottom + 1, x + 1, bottom + 3, 'd');
  if (kind === 'tuck') for (const x of xs) rect(g, x, bottom + 1, x + 1, bottom + 2, 'd');
  if (kind === 'dangleA' || kind === 'dangleB') {
    const sway = kind === 'dangleA' ? 0 : 1;
    rect(g, 8 - sway, bottom + 1, 9 - sway, bottom + 4, 'd');
    rect(g, 12, bottom + 1, 13, bottom + 3 + sway, 'd');
    rect(g, 18, bottom + 1, 19, bottom + 4 - sway, 'd');
    rect(g, 22 + sway, bottom + 1, 23 + sway, bottom + 4, 'd');
  }
}

function eyes(g, top, kind = 'open', dx = 0) {
  const L = 11 + dx, R = 19 + dx, y = top + 3;
  for (const x of [L, R]) {
    switch (kind) {
      case 'open': rect(g, x, y, x + 1, y + 2, 'e'); break;
      case 'glint': rect(g, x, y, x + 1, y + 2, 'e'); px(g, x, y, 'w'); break;
      case 'blink': rect(g, x, y + 2, x + 1, y + 2, 'e'); break;
      case 'down': rect(g, x, y + 1, x + 1, y + 2, 'e'); break;
      case 'closed': rect(g, x - 1, y + 2, x + 2, y + 2, 'e'); break;
      case 'happy': px(g, x - 1, y + 2, 'e'); rect(g, x, y + 1, x + 1, y + 1, 'e'); px(g, x + 2, y + 2, 'e'); break;
      case 'x': px(g, x - 1, y, 'e'); px(g, x + 1, y, 'e'); px(g, x, y + 1, 'e'); px(g, x - 1, y + 2, 'e'); px(g, x + 1, y + 2, 'e'); break;
      default: break;
    }
  }
}

// Arm poses. side = which arm (-1 left, +1 right). `top` = body top row.
function arm(g, side, top, pose = 'nub') {
  const inner = side < 0 ? 7 : 24; // column touching the body
  const o = (n) => inner + side * n; // n pixels further out
  switch (pose) {
    case 'nub': rect(g, Math.min(o(0), o(2)), top + 5, Math.max(o(0), o(2)), top + 6, 'o'); break;
    case 'down': rect(g, Math.min(o(0), o(1)), top + 6, Math.max(o(0), o(1)), top + 8, 'o'); break;
    case 'up': rect(g, Math.min(o(0), o(1)), top - 2, Math.max(o(0), o(1)), top + 5, 'o'); break;
    case 'upout':
      rect(g, Math.min(o(0), o(1)), top + 3, Math.max(o(0), o(1)), top + 5, 'o');
      rect(g, Math.min(o(1), o(2)), top + 0, Math.max(o(1), o(2)), top + 3, 'o');
      rect(g, Math.min(o(2), o(3)), top - 2, Math.max(o(2), o(3)), top + 0, 'o');
      break;
    case 'wave':
      rect(g, Math.min(o(0), o(1)), top + 2, Math.max(o(0), o(1)), top + 5, 'o');
      rect(g, Math.min(o(1), o(2)), top - 2, Math.max(o(1), o(2)), top + 2, 'o');
      break;
    case 'out': rect(g, Math.min(o(0), o(3)), top + 4, Math.max(o(0), o(3)), top + 5, 'o'); break;
    default: break;
  }
}

/**
 * Draw Pip. dy moves the whole critter (negative = up), squash shortens the body from the top.
 */
function pip(g, { dy = 0, squash = 0, eye = 'open', look = 0, armL = 'nub', armR = 'nub', leg = 'stand', blush = false, mouth = false, lift = 0 } = {}) {
  const bottom = 19 + dy;
  const top = 9 + dy + squash;
  shadow(g, lift || -dy);
  legs(g, bottom, leg);
  rect(g, 8, top, 23, bottom, 'o');
  rect(g, 9, top, 22, top, 'h');
  px(g, 8, top, '.');
  px(g, 23, top, '.');
  rect(g, 8, bottom, 23, bottom, 'd');
  rect(g, 23, top + 1, 23, bottom, 'd');
  eyes(g, top, eye, look);
  if (blush) { rect(g, 9, top + 6, 10, top + 6, 'b'); rect(g, 21, top + 6, 22, top + 6, 'b'); }
  if (mouth) rect(g, 15, top + 7, 16, top + 8, 'e');
  arm(g, -1, top, armL);
  arm(g, +1, top, armR);
  return g;
}

function frame(opts) { return rows(pip(blank(), opts)); }

// ---------------------------------------------------------------- working
function laptopFrame(leftUp, rightUp, spark, look = 0) {
  const g = blank();
  const top = 9;
  shadow(g);
  legs(g, 19);
  rect(g, 8, top, 23, 19, 'o');
  rect(g, 9, top, 22, top, 'h');
  px(g, 8, top, '.');
  px(g, 23, top, '.');
  rect(g, 23, top + 1, 23, 19, 'd');
  // eyes look down at the screen and scan left/right while reading
  for (const x of [11 + look, 19 + look]) rect(g, x, 12, x + 1, 13, 'e');
  // arms reach down around the lid to the keys
  rect(g, 6, 14, 7, leftUp ? 18 : 19, 'o');
  rect(g, 24, 14, 25, rightUp ? 18 : 19, 'o');
  rect(g, 6, leftUp ? 18 : 19, 7, leftUp ? 18 : 19, 'd');
  rect(g, 24, rightUp ? 18 : 19, 25, rightUp ? 18 : 19, 'd');
  // the laptop lid (we see its back) and the deck in front
  rect(g, 9, 15, 22, 20, 'L');
  rect(g, 9, 20, 22, 20, 'l');
  rect(g, 22, 15, 22, 20, 'l');
  rect(g, 15, 17, 16, 18, 'g');
  rect(g, 5, 21, 26, 21, 'k');
  rect(g, 5, 22, 26, 22, 'K');
  if (spark === 'L') { px(g, 4, 18, 'g'); px(g, 3, 17, 'g'); px(g, 5, 16, 'g'); }
  if (spark === 'R') { px(g, 27, 18, 'g'); px(g, 28, 17, 'g'); px(g, 26, 16, 'g'); }
  return rows(g);
}

const animations = {
  idle: {
    durations: [900, 500, 900, 500, 260, 110, 1100, 500],
    frames: [
      frame({}),
      frame({ squash: 1 }),
      frame({}),
      frame({ squash: 1 }),
      frame({}),
      frame({ eye: 'blink' }),
      frame({}),
      frame({ squash: 1 }),
    ],
  },
  working: {
    durations: [110, 110, 110, 110, 110, 110, 110, 160],
    frames: [
      laptopFrame(false, true, null, 0),
      laptopFrame(true, false, 'L', 0),
      laptopFrame(false, true, 'R', 1),
      laptopFrame(true, false, null, 1),
      laptopFrame(false, true, null, 1),
      laptopFrame(true, false, 'L', 0),
      laptopFrame(false, false, null, -1),
      laptopFrame(true, true, null, -1),
    ],
  },
  question: {
    durations: [260, 200, 260, 200],
    frames: [
      frame({ eye: 'glint', armR: 'up' }),
      frame({ eye: 'glint', armR: 'wave', dy: -1, lift: 1 }),
      frame({ eye: 'glint', armR: 'up' }),
      frame({ eye: 'glint', armR: 'upout', dy: -1, lift: 1 }),
    ],
  },
  done: {
    durations: [130, 120, 160, 120, 140, 900],
    frames: [
      frame({ eye: 'happy', squash: 1, armL: 'out', armR: 'out', blush: true }),
      frame({ eye: 'happy', dy: -3, armL: 'up', armR: 'up', leg: 'tuck', blush: true }),
      frame({ eye: 'happy', dy: -5, armL: 'upout', armR: 'upout', leg: 'tuck', blush: true }),
      frame({ eye: 'happy', dy: -2, armL: 'up', armR: 'up', blush: true }),
      frame({ eye: 'happy', squash: 1, armL: 'out', armR: 'out', blush: true }),
      frame({ eye: 'happy', armL: 'nub', armR: 'nub', blush: true }),
    ],
  },
  error: {
    durations: [500, 500],
    frames: [
      frame({ eye: 'x', squash: 1, armL: 'down', armR: 'down' }),
      frame({ eye: 'x', squash: 2, armL: 'down', armR: 'down' }),
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
      frame({ eye: 'glint', dy: -3, armL: 'up', armR: 'up', leg: 'dangleA', mouth: true, lift: 6 }),
      frame({ eye: 'glint', dy: -3, armL: 'up', armR: 'up', leg: 'dangleB', mouth: true, lift: 6 }),
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
      frame({ eye: 'open' }),
    ],
  },
  // Idle fidgets: played now and then while Pip is standing around (and when work is finished).
  look: {
    loop: false,
    durations: [500, 700, 250, 700, 400],
    frames: [
      frame({ look: -1 }),
      frame({ look: -2 }),
      frame({}),
      frame({ look: 2 }),
      frame({ look: 1 }),
    ],
  },
  stretch: {
    loop: false,
    durations: [160, 160, 650, 160, 200],
    frames: [
      frame({ squash: 1, armL: 'out', armR: 'out' }),
      frame({ squash: -1, eye: 'closed', armL: 'up', armR: 'up' }),
      frame({ squash: -2, eye: 'closed', armL: 'upout', armR: 'upout' }),
      frame({ squash: -1, eye: 'closed', armL: 'up', armR: 'up' }),
      frame({ squash: 1, eye: 'blink' }),
    ],
  },
  // Whipped! A flinch: X eyes, a hop, then wide awake (then Hustle mode kicks in).
  hit: {
    loop: false,
    durations: [110, 120, 140, 160, 260],
    frames: [
      frame({ eye: 'x', squash: 2, armL: 'out', armR: 'out' }),
      frame({ eye: 'x', dy: -3, armL: 'up', armR: 'up', leg: 'tuck' }),
      frame({ eye: 'glint', dy: -2, armL: 'upout', armR: 'upout', leg: 'tuck' }),
      frame({ eye: 'glint', squash: 1, armL: 'out', armR: 'out' }),
      frame({ eye: 'glint' }),
    ],
  },
  jump: {
    loop: false,
    durations: [110, 110, 140, 120, 110, 160],
    frames: [
      frame({ eye: 'glint', squash: 2, armL: 'out', armR: 'out' }),
      frame({ eye: 'glint', dy: -4, armL: 'up', armR: 'up', leg: 'tuck' }),
      frame({ eye: 'glint', dy: -7, armL: 'upout', armR: 'upout', leg: 'tuck' }),
      frame({ eye: 'glint', dy: -4, armL: 'up', armR: 'up' }),
      frame({ eye: 'glint', squash: 2, armL: 'out', armR: 'out' }),
      frame({ eye: 'glint', armR: 'up' }),
    ],
  },
};

const pet = {
  id: 'pip',
  name: 'Pip',
  author: 'Promptagotchi',
  description: 'A little orange pixel buddy. Types on its laptop while Claude works, waves when it needs you.',
  frameWidth: W,
  frameHeight: H,
  pixelArt: true,
  palette,
  animations,
};

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.join(here, '..', 'pets', 'pip', 'pet.json');
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, JSON.stringify(pet, null, 2) + '\n');
console.log('wrote', path.relative(process.cwd(), out));
