// Renders a text-pixel pet to a PNG contact sheet: one row per state, one column per frame.
//
//   node scripts/preview-pet.mjs pets/pip            -> pets/pip/preview.png
//   node scripts/preview-pet.mjs ~/.promptagotchi/pets/blob out.png --scale 8
//
// No dependencies: includes a tiny PNG encoder.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const args = process.argv.slice(2);
const dir = args.find((a) => !a.startsWith('--') && !a.endsWith('.png')) || 'pets/pip';
const outArg = args.find((a) => a.endsWith('.png'));
const scaleIdx = args.indexOf('--scale');
const S = scaleIdx >= 0 ? Number(args[scaleIdx + 1]) : 6;

const pet = JSON.parse(fs.readFileSync(path.join(dir, 'pet.json'), 'utf8'));
const fw = pet.frameWidth, fh = pet.frameHeight;
const states = Object.entries(pet.animations).filter(([, a]) => Array.isArray(a.frames) && Array.isArray(a.frames[0]));
const cols = Math.max(...states.map(([, a]) => a.frames.length));
const PAD = 2;
const W = (cols * (fw + PAD) + PAD) * S;
const H = (states.length * (fh + PAD) + PAD) * S;
const img = Buffer.alloc(W * H * 4);

function parseColor(c) {
  if (!c || c === 'transparent') return null;
  let m = c.match(/^#([0-9a-f]{6})([0-9a-f]{2})?$/i);
  if (m) return [parseInt(m[1].slice(0, 2), 16), parseInt(m[1].slice(2, 4), 16), parseInt(m[1].slice(4, 6), 16), m[2] ? parseInt(m[2], 16) : 255];
  m = c.match(/^#([0-9a-f]{3})$/i);
  if (m) return [...m[1]].map((h) => parseInt(h + h, 16)).concat(255);
  m = c.match(/^rgba?\(([^)]+)\)$/);
  if (m) { const p = m[1].split(',').map((v) => parseFloat(v)); return [p[0], p[1], p[2], Math.round((p[3] ?? 1) * 255)]; }
  return [255, 0, 255, 255];
}
const pal = Object.fromEntries(Object.entries(pet.palette).map(([k, v]) => [k, parseColor(v)]));

function put(x, y, [r, g, b, a]) {
  const i = (y * W + x) * 4;
  const A = a / 255;
  img[i] = Math.round(r * A + img[i] * (1 - A));
  img[i + 1] = Math.round(g * A + img[i + 1] * (1 - A));
  img[i + 2] = Math.round(b * A + img[i + 2] * (1 - A));
  img[i + 3] = 255;
}

// checkerboard background
for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
  const c = ((Math.floor(x / (S * 2)) + Math.floor(y / (S * 2))) % 2) ? 232 : 246;
  put(x, y, [c, c, c, 255]);
}

states.forEach(([, a], row) => {
  a.frames.forEach((frame, col) => {
    const ox = (PAD + col * (fw + PAD)) * S, oy = (PAD + row * (fh + PAD)) * S;
    frame.forEach((line, y) => [...line].forEach((ch, x) => {
      const c = pal[ch];
      if (!c) return;
      for (let dy = 0; dy < S; dy++) for (let dx = 0; dx < S; dx++) put(ox + x * S + dx, oy + y * S + dy, c);
    }));
  });
});

function png(w, h, rgba) {
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) { raw[y * (w * 4 + 1)] = 0; rgba.copy(raw, y * (w * 4 + 1) + 1, y * w * 4, (y + 1) * w * 4); }
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

const out = outArg || path.join(dir, 'preview.png');
fs.writeFileSync(out, png(W, H, img));
console.log(`wrote ${out}  (rows: ${states.map(([n]) => n).join(', ')})`);
