/*
 * PetPlayer — plays a normalized pet (see src/main/pets.js) on a <canvas>, or an <img> for GIF states.
 * Shared by the overlay and the Pet Studio. Classic script: exposes window.PetPlayer.
 */
(function () {
  'use strict';

  const FALLBACK = {
    working: ['idle'],
    question: ['wave', 'idle'],
    done: ['wave', 'idle'],
    error: ['question', 'idle'],
    sleep: ['idle'],
    drag: ['jump', 'idle'],
    dragLeft: ['drag', 'jump', 'idle'],
    dragRight: ['drag', 'jump', 'idle'],
    wave: ['idle'],
    jump: ['idle'],
    look: ['idle'],
    stretch: ['idle'],
    hit: ['jump', 'question', 'idle'],
    idle: [],
  };

  function parseColor(c) {
    if (!c || c === 'transparent') return null;
    return c;
  }

  function loadImage(url) {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.crossOrigin = 'anonymous';
      img.onload = () => resolve(img);
      img.onerror = () => reject(new Error('Could not load ' + url));
      img.src = url;
    });
  }

  function renderPixels(rows, palette, fw, fh) {
    const c = document.createElement('canvas');
    c.width = fw;
    c.height = fh;
    const ctx = c.getContext('2d');
    rows.forEach((line, y) => {
      for (let x = 0; x < line.length && x < fw; x++) {
        const col = parseColor(palette[line[x]]);
        if (!col) continue;
        ctx.fillStyle = col;
        ctx.fillRect(x, y, 1, 1);
      }
    });
    return c;
  }

  class PetPlayer {
    constructor(host) {
      this.host = host;
      this.canvas = document.createElement('canvas');
      this.canvas.className = 'pet-canvas';
      this.img = document.createElement('img');
      this.img.className = 'pet-gif';
      this.img.draggable = false;
      this.img.style.display = 'none';
      host.append(this.canvas, this.img);
      this.ctx = this.canvas.getContext('2d');
      this.pet = null;
      this.anims = {};
      this.headInset = 0;
      this.height = 104;
      this.state = 'idle';
      this.base = 'idle';
      this.frame = 0;
      this.frameStart = 0;
      this.onceThen = null;
      this.mirror = false;
      this.reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      this._raf = requestAnimationFrame((t) => this._tick(t));
    }

    async setPet(pet) {
      this.pet = pet;
      const anims = {};
      const imgCache = new Map();
      const getImg = (url) => {
        if (!imgCache.has(url)) imgCache.set(url, loadImage(url));
        return imgCache.get(url);
      };
      for (const [state, a] of Object.entries(pet.animations || {})) {
        try {
          if (a.type === 'pixels') {
            anims[state] = { ...a, drawables: a.frames.map((rows) => renderPixels(rows, pet.palette || {}, pet.frameWidth, pet.frameHeight)) };
          } else if (a.type === 'images') {
            const imgs = await Promise.all(a.urls.map(getImg));
            anims[state] = { ...a, drawables: imgs };
          } else if (a.type === 'sheet') {
            const img = await getImg(a.url);
            const fw = pet.frameWidth, fh = pet.frameHeight;
            anims[state] = {
              ...a,
              drawables: Array.from({ length: a.count }, (_, i) => ({ img, sx: (a.col + i) * fw, sy: a.row * fh })),
            };
          } else if (a.type === 'gif') {
            anims[state] = { ...a, drawables: [] };
          }
        } catch (e) {
          console.warn(`[pet] ${state}: ${e.message}`);
        }
      }
      this.anims = anims;
      this.headInset = this._measureHeadInset();
      this._layout();
      this.frame = 0;
      this.frameStart = performance.now();
      this._applyGif();
    }

    /** Fraction of the frame above the pet's head (so bubbles can sit right on top of it). */
    _measureHeadInset() {
      const a = this.anims.idle || Object.values(this.anims)[0];
      if (!a || !a.drawables || !a.drawables.length || !this.pet) return 0;
      const fw = this.pet.frameWidth, fh = this.pet.frameHeight;
      const c = document.createElement('canvas');
      c.width = fw; c.height = fh;
      const ctx = c.getContext('2d');
      const d = a.drawables[0];
      try {
        if (d.img) ctx.drawImage(d.img, d.sx, d.sy, fw, fh, 0, 0, fw, fh);
        else ctx.drawImage(d, 0, 0);
        const data = ctx.getImageData(0, 0, fw, fh).data;
        for (let y = 0; y < fh; y++) for (let x = 0; x < fw; x++) if (data[(y * fw + x) * 4 + 3] > 40) return y / fh;
      } catch {}
      return 0;
    }

    /** Rows (frame pixels) where the idle animation has visible pixels: { top, bottom }. */
    measureContent() {
      const a = this.anims.idle || Object.values(this.anims)[0];
      if (!a || !a.drawables || !a.drawables.length || !this.pet) return null;
      const fw = this.pet.frameWidth, fh = this.pet.frameHeight;
      const c = document.createElement('canvas');
      c.width = fw; c.height = fh;
      const ctx = c.getContext('2d', { willReadFrequently: true });
      let top = Infinity, bottom = -1;
      try {
        for (const d of a.drawables) {
          ctx.clearRect(0, 0, fw, fh);
          if (d.img) ctx.drawImage(d.img, d.sx, d.sy, fw, fh, 0, 0, fw, fh);
          else ctx.drawImage(d, 0, 0);
          const data = ctx.getImageData(0, 0, fw, fh).data;
          for (let y = 0; y < fh; y++) {
            for (let x = 0; x < fw; x++) {
              if (data[(y * fw + x) * 4 + 3] > 40) { top = Math.min(top, y); bottom = Math.max(bottom, y); break; }
            }
          }
        }
      } catch { return null; }
      return bottom >= 0 ? { top, bottom } : null;
    }

    setHeight(px) {
      this.height = px;
      this._layout();
    }

    get displaySize() {
      if (!this.pet) return { w: 0, h: 0, scale: 1 };
      const fh = this.pet.frameHeight, fw = this.pet.frameWidth;
      let scale = this.height / fh;
      if (this.pet.pixelArt) scale = Math.max(1, Math.round(scale));
      return { w: Math.round(fw * scale), h: Math.round(fh * scale), scale };
    }

    _layout() {
      const { w, h } = this.displaySize;
      const dpr = window.devicePixelRatio || 1;
      this.canvas.style.width = w + 'px';
      this.canvas.style.height = h + 'px';
      this.canvas.width = Math.round(w * dpr);
      this.canvas.height = Math.round(h * dpr);
      this.img.style.height = h + 'px';
      this.ctx.imageSmoothingEnabled = !(this.pet && this.pet.pixelArt);
      this._draw();
    }

    resolve(state) {
      if (this.anims[state]) return state;
      for (const f of FALLBACK[state] || ['idle']) if (this.anims[f]) return f;
      if (state === 'dragLeft' && this.anims.dragRight) return 'dragRight';
      if (state === 'dragRight' && this.anims.dragLeft) return 'dragLeft';
      return Object.keys(this.anims)[0] || null;
    }

    has(state) {
      return !!this.anims[state];
    }

    /** Set the looping base state. */
    setState(state) {
      this.base = state;
      if (this.onceThen) return; // a one-shot is playing; base resumes after it
      this._switch(state);
    }

    /** Play a one-shot animation (wave, jump), then go back to the base state. */
    playOnce(state) {
      if (!this.anims[state] || this.reducedMotion) return;
      this.onceThen = true;
      this._switch(state, true);
    }

    _switch(state, force) {
      const resolved = this.resolve(state);
      // Mirror a one-sided drag animation when moving the other way.
      this.mirror = (state === 'dragLeft' && resolved === 'dragRight') || (state === 'dragRight' && resolved === 'dragLeft');
      if (!force && resolved === this.state) return;
      this.state = resolved;
      this.frame = 0;
      this.frameStart = performance.now();
      this._applyGif();
      this._draw();
    }

    _applyGif() {
      const a = this.anims[this.state];
      const isGif = a && a.type === 'gif';
      this.canvas.style.display = isGif ? 'none' : '';
      this.img.style.display = isGif ? '' : 'none';
      if (isGif && this.img.getAttribute('src') !== a.url) this.img.src = a.url;
      this.img.style.transform = this.mirror ? 'scaleX(-1)' : '';
    }

    _tick(t) {
      this._raf = requestAnimationFrame((tt) => this._tick(tt));
      const a = this.anims[this.state];
      if (!a || this.reducedMotion) return;
      const n = a.type === 'gif' ? 1 : a.drawables.length;
      const dur = ((a.durations && a.durations[this.frame]) || 150) / (this.speed || 1); // Hustle mode speeds it up
      if (t - this.frameStart < dur) return;
      this.frameStart = t;
      if (this.frame + 1 < n) {
        this.frame++;
      } else if (this.onceThen) {
        this.onceThen = null;
        this._switch(this.base, true);
        return;
      } else if (a.loop === false) {
        return;
      } else {
        this.frame = 0;
      }
      this._draw();
    }

    _draw() {
      const a = this.anims[this.state];
      const ctx = this.ctx;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
      if (!a || a.type === 'gif' || !this.pet) return;
      const d = a.drawables[Math.min(this.frame, a.drawables.length - 1)];
      if (!d) return;
      const fw = this.pet.frameWidth, fh = this.pet.frameHeight;
      ctx.imageSmoothingEnabled = !this.pet.pixelArt;
      if (this.mirror) { ctx.translate(this.canvas.width, 0); ctx.scale(-1, 1); }
      if (d.img) ctx.drawImage(d.img, d.sx, d.sy, fw, fh, 0, 0, this.canvas.width, this.canvas.height);
      else ctx.drawImage(d, 0, 0, fw, fh, 0, 0, this.canvas.width, this.canvas.height);
    }

    destroy() {
      cancelAnimationFrame(this._raf);
      this.canvas.remove();
      this.img.remove();
    }
  }

  window.PetPlayer = PetPlayer;
})();
