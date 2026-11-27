/*
 * Whip mode: a pixel whip that trails your cursor (a little rope simulation), drawn on a
 * click-through overlay covering the screen your cursor is on. Bring it within reach of your
 * pet and the lash swings at it by itself (no ring: it just happens); when it lands, it cracks
 * and the pet flinches into Hustle mode. Fast swings get a whiplash smear and speed lines.
 * Nothing makes a sound unless it actually hits the pet.
 */
(function () {
  'use strict';

  const canvas = document.getElementById('c');
  const ctx = canvas.getContext('2d');
  const N = 22;          // rope points
  const SEG = 9;         // px between points
  const HANDLE = { dx: 16, dy: 18 };
  const CRACK_SPEED = 1500; // px/s at the tip
  const sparks = [];
  let hand = null;
  let pet = null;
  let pts = [];
  let lastTip = null;
  let lastT = performance.now();
  let lastCrack = 0;
  let lastStrike = 0;
  let lastStrikeHand = null;
  let strike = null; // { t0, from, to, dur } while the lash is swinging at the pet
  const trail = [];  // recent lash shapes, for the whiplash motion blur
  const shocks = []; // crack shockwaves and snap streaks
  let vel = { x: 0, y: 0 }; // tip velocity (px/s)
  let sound = true;
  let audio = null;

  function fit() {
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(innerWidth * dpr);
    canvas.height = Math.round(innerHeight * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }
  addEventListener('resize', fit);
  fit();

  function reset(x, y) {
    pts = Array.from({ length: N }, (_, i) => ({ x: x + HANDLE.dx, y: y + HANDLE.dy + i * SEG, px: x + HANDLE.dx, py: y + HANDLE.dy + i * SEG }));
    lastTip = null;
  }

  /** Only ever played when the lash lands on the pet. */
  function crackSound(power) {
    if (!sound) return;
    try {
      audio = audio || new AudioContext();
      const len = Math.floor(audio.sampleRate * 0.12);
      const buf = audio.createBuffer(1, len, audio.sampleRate);
      const d = buf.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 6);
      const src = audio.createBufferSource();
      src.buffer = buf;
      const hp = audio.createBiquadFilter();
      hp.type = 'highpass';
      hp.frequency.value = 1800;
      const g = audio.createGain();
      g.gain.value = Math.min(0.5, 0.18 + power * 0.1);
      src.connect(hp).connect(g).connect(audio.destination);
      src.start();
    } catch {}
  }

  function burst(x, y, hit) {
    shocks.push({ x, y, t0: performance.now(), hit, dx: vel.x, dy: vel.y });
    for (let i = 0; i < (hit ? 18 : 8); i++) {
      const a = Math.random() * Math.PI * 2;
      const v = (hit ? 3 : 2) + Math.random() * 3;
      sparks.push({ x, y, vx: Math.cos(a) * v, vy: Math.sin(a) * v, life: 1, c: hit ? (i % 3 ? '#ffd23b' : '#ffffff') : '#f3e2b8' });
    }
  }

  function step(t) {
    requestAnimationFrame(step);
    const dt = Math.min(0.05, (t - lastT) / 1000);
    lastT = t;
    ctx.clearRect(0, 0, innerWidth, innerHeight);
    if (!hand) return;
    if (!pts.length) reset(hand.x, hand.y);

    // Auto-strike: once the handle is within the lash's reach of the pet, the lash swings at it
    // by itself. Only a lash that actually lands on the pet counts (and makes a sound).
    const handleTip = { x: hand.x + HANDLE.dx, y: hand.y + HANDLE.dy };
    const target = pet ? { x: pet.x + pet.w / 2, y: pet.y + pet.h * 0.62 } : null;
    const reach = (N - 1) * SEG * 0.9;
    const dToPet = target ? Math.hypot(target.x - handleTip.x, target.y - handleTip.y) : Infinity;
    const inRange = dToPet < reach;
    if (target && !strike && inRange && t - lastStrike > 650) {
      const moved = !lastStrikeHand || Math.hypot(hand.x - lastStrikeHand.x, hand.y - lastStrikeHand.y) > 24;
      if (moved) strike = { t0: t, from: { x: pts[N - 1].x, y: pts[N - 1].y }, to: target, dur: 180 };
    }
    let pin = null;
    if (strike) {
      const k = Math.min(1, (t - strike.t0) / strike.dur);
      const e = k * k; // speeds up into the hit
      pin = {
        x: strike.from.x + (strike.to.x - strike.from.x) * e,
        y: strike.from.y + (strike.to.y - strike.from.y) * e - Math.sin(Math.PI * k) * 60,
      };
    }

    // Verlet rope: the first point is the end of the handle, the rest swing under gravity.
    const p0 = pts[0];
    p0.px = p0.x; p0.py = p0.y;
    p0.x = handleTip.x; p0.y = handleTip.y;
    for (let i = 1; i < N; i++) {
      const p = pts[i];
      const vx = (p.x - p.px) * 0.985, vy = (p.y - p.py) * 0.985 + 0.55;
      p.px = p.x; p.py = p.y;
      p.x += vx; p.y += vy;
    }
    for (let k = 0; k < 8; k++) {
      for (let i = 1; i < N; i++) {
        const a = pts[i - 1], b = pts[i];
        const dx = b.x - a.x, dy = b.y - a.y;
        const dist = Math.hypot(dx, dy) || 0.001;
        const diff = (dist - SEG) / dist;
        if (i === 1) { b.x -= dx * diff; b.y -= dy * diff; }
        else { a.x += dx * diff * 0.5; a.y += dy * diff * 0.5; b.x -= dx * diff * 0.5; b.y -= dy * diff * 0.5; }
      }
      pts[0].x = handleTip.x; pts[0].y = handleTip.y;
      if (pin) { pts[N - 1].x = pin.x; pts[N - 1].y = pin.y; }
    }

    const tip = pts[N - 1];
    const tipSpeed = lastTip && dt > 0 ? Math.hypot(tip.x - lastTip.x, tip.y - lastTip.y) / dt : 0;
    if (lastTip && dt > 0) vel = { x: (tip.x - lastTip.x) / dt, y: (tip.y - lastTip.y) / dt };
    const land = (speed) => {
      burst(tip.x, tip.y, true);
      crackSound(1.4);
      window.pet.whipHit({ speed: Math.round(speed) });
      lastStrike = t;
      lastStrikeHand = { x: hand.x, y: hand.y };
    };
    if (strike && t - strike.t0 >= strike.dur) {
      // The lash landed on the pet.
      land(2000);
      tip.px = tip.x + (tip.x - handleTip.x) * 0.12; // a little recoil
      tip.py = tip.y + 6;
      strike = null;
    } else if (!strike && lastTip && dt > 0) {
      // A fast snap of your own: counts only if the tip is on the pet; elsewhere it's a silent puff.
      const speed = tipSpeed;
      if (speed > CRACK_SPEED && t - lastCrack > 280) {
        lastCrack = t;
        const onPet = pet && tip.x > pet.x - 8 && tip.x < pet.x + pet.w + 8 && tip.y > pet.y - 8 && tip.y < pet.y + pet.h + 8;
        if (onPet && t - lastStrike > 300) land(speed);
        else burst(tip.x, tip.y, false);
      }
    }
    lastTip = { x: tip.x, y: tip.y };

    // Whiplash: ghost copies of the lash trail behind a fast swing.
    const fast = tipSpeed > 450 || !!strike;
    trail.push(pts.filter((_, i) => i % 2 === 0 || i === N - 1).map((p) => ({ x: p.x, y: p.y })));
    if (trail.length > 7) trail.shift();
    if (!fast && trail.length) trail.splice(0, 2);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    trail.forEach((shape, k) => {
      const a = (0.05 + 0.2 * (k / trail.length)) * Math.min(1, tipSpeed / 1500 + (strike ? 0.6 : 0));
      if (a < 0.02) return;
      ctx.strokeStyle = `rgba(255, 236, 200, ${a})`;
      ctx.lineWidth = 3;
      ctx.beginPath();
      shape.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
      ctx.stroke();
    });

    // Draw: handle, then the lash tapering to a light cracker at the tip.
    ctx.strokeStyle = '#3d2412';
    ctx.lineWidth = 7;
    ctx.beginPath(); ctx.moveTo(hand.x + 3, hand.y + 4); ctx.lineTo(p0.x, p0.y); ctx.stroke();
    ctx.strokeStyle = '#7a4a26';
    ctx.lineWidth = 5;
    ctx.beginPath(); ctx.moveTo(hand.x + 3, hand.y + 4); ctx.lineTo(p0.x, p0.y); ctx.stroke();
    ctx.strokeStyle = '#d9a35c';
    ctx.lineWidth = 2;
    for (let s = 0.25; s < 0.9; s += 0.22) {
      const x = hand.x + 3 + (p0.x - hand.x - 3) * s, y = hand.y + 4 + (p0.y - hand.y - 4) * s;
      ctx.beginPath(); ctx.moveTo(x - 2, y + 2); ctx.lineTo(x + 2, y - 2); ctx.stroke();
    }
    for (let i = 1; i < N; i++) {
      const a = pts[i - 1], b = pts[i];
      ctx.strokeStyle = i > N - 3 ? '#e9cf98' : '#8a5a2b';
      ctx.lineWidth = Math.max(1.2, 3.6 - (i / N) * 2.6);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
    }

    // Speed lines streaming off the tip while it flies.
    if (tipSpeed > 900) {
      const len = Math.hypot(vel.x, vel.y) || 1;
      const ux = -vel.x / len, uy = -vel.y / len; // pointing back along the swing
      const k = Math.min(1, (tipSpeed - 900) / 1600);
      ctx.strokeStyle = `rgba(255, 255, 255, ${0.25 + 0.45 * k})`;
      ctx.lineWidth = 1.5;
      for (const off of [-6, 0, 6]) {
        const ox = -uy * off, oy = ux * off;
        const l = 14 + 26 * k - Math.abs(off);
        ctx.beginPath();
        ctx.moveTo(tip.x + ox + ux * 6, tip.y + oy + uy * 6);
        ctx.lineTo(tip.x + ox + ux * (6 + l), tip.y + oy + uy * (6 + l));
        ctx.stroke();
      }
    }

    // Cracks: a shockwave ring, and on a hit a bright jagged snap streak.
    for (let i = shocks.length - 1; i >= 0; i--) {
      const sh = shocks[i];
      const age = (t - sh.t0) / (sh.hit ? 380 : 260);
      if (age >= 1) { shocks.splice(i, 1); continue; }
      ctx.strokeStyle = `rgba(255, 250, 230, ${(1 - age) * (sh.hit ? 0.9 : 0.35)})`;
      ctx.lineWidth = sh.hit ? 2.5 : 1.5;
      ctx.beginPath();
      ctx.arc(sh.x, sh.y, (sh.hit ? 6 : 3) + age * (sh.hit ? 46 : 18), 0, Math.PI * 2);
      ctx.stroke();
      if (sh.hit && age < 0.45) {
        const len = Math.hypot(sh.dx, sh.dy) || 1;
        const ux = sh.dx / len || 1, uy = sh.dy / len;
        ctx.strokeStyle = `rgba(255, 246, 200, ${1 - age / 0.45})`;
        ctx.lineWidth = 2.5;
        ctx.beginPath();
        ctx.moveTo(sh.x, sh.y);
        for (let j = 1; j <= 5; j++) {
          const jit = (j % 2 ? 1 : -1) * 5;
          ctx.lineTo(sh.x + ux * j * 9 - uy * jit, sh.y + uy * j * 9 + ux * jit);
        }
        ctx.stroke();
      }
    }

    // Sparks from a crack.
    for (let i = sparks.length - 1; i >= 0; i--) {
      const s = sparks[i];
      s.x += s.vx; s.y += s.vy; s.vy += 0.15; s.life -= 0.045;
      if (s.life <= 0) { sparks.splice(i, 1); continue; }
      ctx.globalAlpha = s.life;
      ctx.fillStyle = s.c;
      ctx.fillRect(Math.round(s.x), Math.round(s.y), 3, 3);
      ctx.globalAlpha = 1;
    }
  }
  requestAnimationFrame(step);

  window.pet.on('whip:frame', (f) => {
    if (!hand) reset(f.x, f.y);
    hand = { x: f.x, y: f.y };
    pet = f.pet;
    sound = f.sound !== false;
  });
  window.pet.on('whip:reset', () => { hand = null; pts = []; strike = null; trail.length = 0; shocks.length = 0; });
})();
