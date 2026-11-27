/* Pet Studio: a small pixel editor for Promptagotchi's text-pixel pet format. */
(function () {
  'use strict';

  const $ = (s) => document.querySelector(s);
  const STATES = ['idle', 'working', 'question', 'done', 'error', 'sleep', 'drag', 'wave', 'jump', 'look', 'stretch', 'hit'];
  const CHARS = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#*+=@%&$';
  const ONCE = new Set(['wave', 'jump', 'look', 'stretch', 'hit']);

  let pets = [];
  let curKey = null;
  let curSource = null;
  let doc = null;
  let readonly = false;
  let state = 'idle';
  let frameIdx = 0;
  let tool = 'pencil';
  let color = null;
  let undo = [];
  let redo = [];
  let dirty = false;
  let cell = 16;
  let previewState = null;

  const canvas = $('#canvas');
  const ctx = canvas.getContext('2d');
  const player = new PetPlayer($('#preview-pet'));

  // ------------------------------------------------------------- helpers
  function toast(msg) {
    const t = $('#toast');
    t.textContent = msg;
    t.classList.add('show');
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => t.classList.remove('show'), 2400);
  }
  const clone = (o) => JSON.parse(JSON.stringify(o));
  const blankFrame = (w, h) => Array.from({ length: h }, () => '.'.repeat(w));
  const anim = () => doc && doc.animations[state];
  const frame = () => anim() && anim().frames[frameIdx];

  function durationsOf(a) {
    if (Array.isArray(a.durations) && a.durations.length) {
      const d = a.durations.slice(0, a.frames.length);
      while (d.length < a.frames.length) d.push(d[d.length - 1] || 150);
      return d;
    }
    const fps = a.fps || doc.fps || 6;
    return a.frames.map(() => Math.round(1000 / fps));
  }

  function normalize() {
    const animations = {};
    for (const [k, a] of Object.entries(doc.animations)) {
      if (!Array.isArray(a.frames) || !a.frames.length || typeof a.frames[0] === 'string') continue;
      animations[k] = { type: 'pixels', frames: a.frames, durations: durationsOf(a), loop: a.loop !== undefined ? a.loop : !ONCE.has(k) };
    }
    return { key: 'studio', frameWidth: doc.frameWidth, frameHeight: doc.frameHeight, pixelArt: true, palette: doc.palette, animations };
  }

  function colorOf(ch) {
    const c = doc.palette[ch];
    return !c || c === 'transparent' || ch === '.' || ch === ' ' ? null : c;
  }

  function markDirty() {
    dirty = true;
    document.title = 'Pet Studio — ' + (doc.name || doc.id) + ' •';
    schedulePreview();
  }

  // --------------------------------------------------------------- pets
  async function refreshList() {
    pets = await window.studio.list();
    const el = $('#pet-list');
    el.innerHTML = '';
    const groups = [['builtin', 'Built in'], ['user', 'Your pets'], ['codex', 'Codex'], ['petdex', 'Petdex']];
    for (const [src, label] of groups) {
      const g = pets.filter((p) => p.source === src);
      if (!g.length) continue;
      const h = document.createElement('div');
      h.className = 'group';
      h.textContent = label;
      el.append(h);
      for (const p of g) {
        const row = document.createElement('div');
        row.className = 'pet' + (p.key === curKey ? ' on' : '');
        row.innerHTML = `<span></span><small></small>`;
        row.firstChild.textContent = p.name;
        row.lastChild.textContent = p.error ? 'broken' : p.format === 'codex' ? 'sheet' : '';
        row.onclick = () => openPet(p.key);
        el.append(row);
      }
    }
  }

  async function openPet(key) {
    if (dirty && !confirm('Discard unsaved changes?')) return;
    let res;
    try {
      res = await window.studio.read(key);
    } catch (e) {
      toast(e.message);
      return;
    }
    curKey = key;
    curSource = res.pet.source;
    const isPixel = !res.json.spritesheetPath && res.json.animations && Object.values(res.json.animations).some((a) => Array.isArray(a.frames) && Array.isArray(a.frames[0]));
    readonly = !isPixel;
    doc = isPixel ? clone(res.json) : null;
    dirty = false;
    undo = []; redo = [];
    $('#readonly').hidden = !readonly;
    $('#workbench').style.display = readonly ? 'none' : '';
    $('#states').style.display = readonly ? 'none' : '';
    if (readonly) {
      $('#readonly').innerHTML = `<b>${escapeHtml(res.pet.name)}</b> is drawn with images (${res.json.spritesheetPath ? 'a sprite sheet' : 'image files'}), so edit it in your art app. You can preview every state on the right. See <code>docs/making-pets.md</code> for the image formats.`;
      fillMeta(res.json, res.pet);
      await player.setPet(res.normalized);
      player.setHeight(Math.min(150, res.normalized.frameHeight * (res.normalized.pixelArt ? 4 : 0.7)));
      buildPreviewStates(Object.keys(res.normalized.animations));
      showWarnings(res.normalized.warnings || []);
    } else {
      fillMeta(doc, res.pet);
      state = doc.animations.idle ? 'idle' : Object.keys(doc.animations)[0] || 'idle';
      frameIdx = 0;
      color = Object.keys(doc.palette).find((k) => k !== '.' && k !== ' ') || null;
      renderAll();
      showWarnings(res.normalized.warnings || []);
    }
    document.title = 'Pet Studio — ' + res.pet.name;
    refreshList();
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  }

  function fillMeta(j, pet) {
    $('#f-name').value = j.name || j.displayName || '';
    $('#f-id').value = j.id || (pet && pet.id) || '';
    $('#f-author').value = j.author || '';
    $('#f-desc').value = j.description || '';
    $('#f-w').value = j.frameWidth || 192;
    $('#f-h').value = j.frameHeight || 208;
    for (const id of ['#f-name', '#f-id', '#f-author', '#f-desc', '#f-w', '#f-h', '#resize', '#save', '#use']) $(id).disabled = readonly;
  }

  function newBlank() {
    if (dirty && !confirm('Discard unsaved changes?')) return;
    const w = 32, h = 28;
    doc = {
      id: 'my-pet',
      name: 'My Pet',
      author: '',
      description: '',
      frameWidth: w,
      frameHeight: h,
      pixelArt: true,
      palette: { '.': 'transparent', a: '#7cc4ff', b: '#3d7bd9', c: '#1b1b2b', d: '#ffffff' },
      animations: { idle: { durations: [500], frames: [blankFrame(w, h)] } },
    };
    curKey = null;
    curSource = 'user';
    readonly = false;
    $('#readonly').hidden = true;
    $('#workbench').style.display = '';
    $('#states').style.display = '';
    fillMeta(doc);
    state = 'idle';
    frameIdx = 0;
    color = 'a';
    undo = []; redo = [];
    dirty = true;
    renderAll();
    refreshList();
    toast('Blank pet ready. Draw an idle frame to start.');
  }

  function copyCurrent() {
    if (!doc) return toast('Only pixel pets can be copied here.');
    doc = clone(doc);
    doc.id = (doc.id || 'pet') + '-copy';
    doc.name = (doc.name || 'Pet') + ' (copy)';
    curKey = null;
    curSource = 'user';
    fillMeta(doc);
    markDirty();
    refreshList();
    toast('Copied. Save to keep it.');
  }

  // ------------------------------------------------------------- states
  function renderStates() {
    const nav = $('#states');
    nav.innerHTML = '';
    for (const s of STATES) {
      const b = document.createElement('button');
      const has = !!doc.animations[s];
      b.className = (s === state ? 'on ' : '') + (has ? '' : 'missing');
      b.innerHTML = `${s}${has ? '<span class="has"></span>' : ''}`;
      b.title = has ? `${doc.animations[s].frames.length} frame(s)` : 'Not drawn yet: click to start it from your idle frame';
      b.onclick = () => {
        if (!doc.animations[s]) {
          pushUndo();
          const base = doc.animations.idle ? clone(doc.animations.idle.frames[0]) : blankFrame(doc.frameWidth, doc.frameHeight);
          doc.animations[s] = { durations: [ONCE.has(s) ? 140 : 300], frames: [base] };
          if (ONCE.has(s)) doc.animations[s].loop = false;
          markDirty();
        }
        state = s;
        frameIdx = 0;
        renderAll();
      };
      nav.append(b);
    }
  }

  // ------------------------------------------------------------- canvas
  function fitCanvas() {
    const wrap = $('#canvas-wrap');
    const w = doc.frameWidth, h = doc.frameHeight;
    cell = Math.max(4, Math.min(28, Math.floor(Math.min((wrap.clientWidth - 24) / w, (wrap.clientHeight - 24) / h))));
    const dpr = window.devicePixelRatio || 1;
    canvas.width = w * cell * dpr;
    canvas.height = h * cell * dpr;
    canvas.style.width = w * cell + 'px';
    canvas.style.height = h * cell + 'px';
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  function drawCanvas() {
    if (!doc || !anim()) return;
    const w = doc.frameWidth, h = doc.frameHeight;
    ctx.clearRect(0, 0, w * cell, h * cell);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      ctx.fillStyle = (x + y) % 2 ? '#2b282d' : '#323035';
      ctx.fillRect(x * cell, y * cell, cell, cell);
    }
    const onion = $('#onion').checked && frameIdx > 0 ? anim().frames[frameIdx - 1] : null;
    if (onion) {
      ctx.globalAlpha = 0.22;
      paintFrame(onion);
      ctx.globalAlpha = 1;
    }
    paintFrame(frame());
    if ($('#grid').checked && cell >= 8) {
      ctx.strokeStyle = 'rgba(255,255,255,0.06)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let x = 0; x <= w; x++) { ctx.moveTo(x * cell + 0.5, 0); ctx.lineTo(x * cell + 0.5, h * cell); }
      for (let y = 0; y <= h; y++) { ctx.moveTo(0, y * cell + 0.5); ctx.lineTo(w * cell, y * cell + 0.5); }
      ctx.stroke();
    }
  }

  function paintFrame(rows) {
    rows.forEach((line, y) => {
      for (let x = 0; x < line.length; x++) {
        const c = colorOf(line[x]);
        if (!c) continue;
        ctx.fillStyle = c;
        ctx.fillRect(x * cell, y * cell, cell, cell);
      }
    });
  }

  function getPx(x, y) { return frame()[y][x]; }
  function setPx(x, y, ch) {
    const rows = frame();
    if (y < 0 || y >= rows.length || x < 0 || x >= rows[y].length) return;
    if (rows[y][x] === ch) return;
    rows[y] = rows[y].slice(0, x) + ch + rows[y].slice(x + 1);
  }

  function floodFill(x, y, ch) {
    const target = getPx(x, y);
    if (target === ch) return;
    const w = doc.frameWidth, h = doc.frameHeight;
    const stack = [[x, y]];
    while (stack.length) {
      const [cx, cy] = stack.pop();
      if (cx < 0 || cy < 0 || cx >= w || cy >= h || getPx(cx, cy) !== target) continue;
      setPx(cx, cy, ch);
      stack.push([cx + 1, cy], [cx - 1, cy], [cx, cy + 1], [cx, cy - 1]);
    }
  }

  function cellAt(e) {
    const r = canvas.getBoundingClientRect();
    return [Math.floor((e.clientX - r.left) / cell), Math.floor((e.clientY - r.top) / cell)];
  }

  let painting = null;
  canvas.addEventListener('pointerdown', (e) => {
    if (!doc || !anim()) return;
    const [x, y] = cellAt(e);
    if (tool === 'picker' || e.altKey) {
      const ch = getPx(x, y);
      if (ch && ch !== '.') { color = ch; setTool('pencil'); renderPalette(); }
      return;
    }
    pushUndo();
    if (tool === 'fill') {
      floodFill(x, y, color || '.');
      afterEdit();
      return;
    }
    canvas.setPointerCapture(e.pointerId);
    const ch = tool === 'eraser' || e.button === 2 ? '.' : color || '.';
    painting = { last: [x, y], ch };
    setPx(x, y, ch);
    drawCanvas();
  });
  canvas.addEventListener('pointermove', (e) => {
    if (!painting) return;
    const [x, y] = cellAt(e);
    let [x0, y0] = painting.last;
    // Bresenham so fast strokes don't leave gaps.
    const dx = Math.abs(x - x0), dy = -Math.abs(y - y0), sx = x0 < x ? 1 : -1, sy = y0 < y ? 1 : -1;
    let err = dx + dy;
    for (;;) {
      setPx(x0, y0, painting.ch);
      if (x0 === x && y0 === y) break;
      const e2 = 2 * err;
      if (e2 >= dy) { err += dy; x0 += sx; }
      if (e2 <= dx) { err += dx; y0 += sy; }
    }
    painting.last = [x, y];
    drawCanvas();
  });
  const endPaint = () => { if (painting) { painting = null; afterEdit(); } };
  canvas.addEventListener('pointerup', endPaint);
  canvas.addEventListener('pointercancel', endPaint);
  canvas.addEventListener('contextmenu', (e) => e.preventDefault());

  function afterEdit() {
    drawCanvas();
    renderFrames();
    markDirty();
  }

  // ------------------------------------------------------------- undo
  function pushUndo() {
    undo.push(JSON.stringify({ doc, state, frameIdx }));
    if (undo.length > 200) undo.shift();
    redo = [];
  }
  function restore(snap) {
    const s = JSON.parse(snap);
    doc = s.doc;
    state = s.state;
    frameIdx = s.frameIdx;
    renderAll();
    markDirty();
  }
  function doUndo() { if (!undo.length) return; redo.push(JSON.stringify({ doc, state, frameIdx })); restore(undo.pop()); }
  function doRedo() { if (!redo.length) return; undo.push(JSON.stringify({ doc, state, frameIdx })); restore(redo.pop()); }

  // ------------------------------------------------------------ palette
  function renderPalette() {
    const el = $('#palette');
    el.innerHTML = '';
    const eraser = document.createElement('div');
    eraser.className = 'swatch' + (tool === 'eraser' ? ' on' : '');
    eraser.title = 'Transparent (eraser)';
    eraser.innerHTML = '<b>.</b>';
    eraser.onclick = () => setTool('eraser');
    el.append(eraser);
    for (const [ch, c] of Object.entries(doc.palette)) {
      if (ch === '.' || ch === ' ') continue;
      const sw = document.createElement('div');
      sw.className = 'swatch' + (ch === color && tool !== 'eraser' ? ' on' : '');
      sw.title = `"${ch}" ${c}  ·  click again to change, alt-click to remove if unused`;
      sw.innerHTML = `<i style="background:${c}"></i><b>${escapeHtml(ch)}</b>`;
      sw.onclick = (e) => {
        if (e.altKey) return removeColor(ch);
        if (color === ch && tool !== 'eraser') {
          const input = document.createElement('input');
          input.type = 'color';
          input.value = /^#[0-9a-f]{6}$/i.test(c) ? c : '#ffffff';
          input.oninput = () => { doc.palette[ch] = input.value; sw.firstChild.style.background = input.value; drawCanvas(); renderFrames(); markDirty(); };
          input.click();
          return;
        }
        color = ch;
        if (tool === 'eraser' || tool === 'picker') tool = 'pencil';
        renderTools();
        renderPalette();
      };
      el.append(sw);
    }
  }

  function removeColor(ch) {
    const used = Object.values(doc.animations).some((a) => a.frames.some((f) => f.some((r) => r.includes(ch))));
    if (used) return toast(`"${ch}" is still used in a frame.`);
    pushUndo();
    delete doc.palette[ch];
    if (color === ch) color = null;
    renderPalette();
    markDirty();
  }

  $('#new-color').addEventListener('change', (e) => {
    const ch = [...CHARS].find((c) => !(c in doc.palette));
    if (!ch) return toast('Palette is full.');
    pushUndo();
    doc.palette[ch] = e.target.value;
    color = ch;
    setTool('pencil');
    renderPalette();
    markDirty();
  });

  // ------------------------------------------------------------- tools
  function setTool(t) {
    tool = t;
    renderTools();
    renderPalette();
  }
  function renderTools() {
    document.querySelectorAll('[data-tool]').forEach((b) => b.classList.toggle('on', b.dataset.tool === tool));
  }
  document.querySelectorAll('[data-tool]').forEach((b) => b.addEventListener('click', () => setTool(b.dataset.tool)));

  function transform(fn) {
    pushUndo();
    anim().frames[frameIdx] = fn(frame());
    afterEdit();
  }
  const W = () => doc.frameWidth;
  $('#flip').onclick = () => transform((rows) => rows.map((r) => [...r].reverse().join('')));
  $('#shift-up').onclick = () => transform((rows) => [...rows.slice(1), '.'.repeat(W())]);
  $('#shift-down').onclick = () => transform((rows) => ['.'.repeat(W()), ...rows.slice(0, -1)]);
  $('#shift-left').onclick = () => transform((rows) => rows.map((r) => r.slice(1) + '.'));
  $('#shift-right').onclick = () => transform((rows) => rows.map((r) => '.' + r.slice(0, -1)));
  $('#clear').onclick = () => transform(() => blankFrame(doc.frameWidth, doc.frameHeight));
  $('#undo').onclick = doUndo;
  $('#redo').onclick = doRedo;
  $('#onion').onchange = drawCanvas;
  $('#grid').onchange = drawCanvas;

  // ------------------------------------------------------------- frames
  function renderFrames() {
    const el = $('#frames');
    el.innerHTML = '';
    const a = anim();
    if (!a) return;
    const s = Math.max(2, Math.floor(56 / doc.frameHeight));
    a.frames.forEach((rows, i) => {
      const c = document.createElement('canvas');
      c.width = doc.frameWidth * s;
      c.height = doc.frameHeight * s;
      c.className = i === frameIdx ? 'on' : '';
      const cx = c.getContext('2d');
      rows.forEach((line, y) => { for (let x = 0; x < line.length; x++) { const col = colorOf(line[x]); if (col) { cx.fillStyle = col; cx.fillRect(x * s, y * s, s, s); } } });
      c.title = `Frame ${i + 1}`;
      c.onclick = () => { frameIdx = i; drawCanvas(); renderFrames(); syncFrameInputs(); };
      el.append(c);
    });
    syncFrameInputs();
  }

  function syncFrameInputs() {
    const a = anim();
    if (!a) return;
    $('#f-ms').value = durationsOf(a)[frameIdx];
    $('#f-loop').checked = a.loop !== undefined ? a.loop : !ONCE.has(state);
    $('#f-del').textContent = a.frames.length === 1 && state !== 'idle' ? 'Delete state' : 'Delete';
  }

  function ensureDurations(a) {
    a.durations = durationsOf(a);
    delete a.fps;
  }

  $('#f-add').onclick = () => { pushUndo(); const a = anim(); ensureDurations(a); a.frames.splice(frameIdx + 1, 0, blankFrame(doc.frameWidth, doc.frameHeight)); a.durations.splice(frameIdx + 1, 0, a.durations[frameIdx]); frameIdx++; afterEdit(); };
  $('#f-dup').onclick = () => { pushUndo(); const a = anim(); ensureDurations(a); a.frames.splice(frameIdx + 1, 0, clone(frame())); a.durations.splice(frameIdx + 1, 0, a.durations[frameIdx]); frameIdx++; afterEdit(); };
  $('#f-left').onclick = () => moveFrame(-1);
  $('#f-right').onclick = () => moveFrame(1);
  function moveFrame(d) {
    const a = anim();
    const j = frameIdx + d;
    if (j < 0 || j >= a.frames.length) return;
    pushUndo();
    ensureDurations(a);
    [a.frames[frameIdx], a.frames[j]] = [a.frames[j], a.frames[frameIdx]];
    [a.durations[frameIdx], a.durations[j]] = [a.durations[j], a.durations[frameIdx]];
    frameIdx = j;
    afterEdit();
  }
  $('#f-del').onclick = () => {
    const a = anim();
    pushUndo();
    if (a.frames.length === 1) {
      if (state === 'idle') return toast('Every pet needs at least one idle frame.');
      delete doc.animations[state];
      state = 'idle';
      frameIdx = 0;
      renderAll();
      markDirty();
      return;
    }
    ensureDurations(a);
    a.frames.splice(frameIdx, 1);
    a.durations.splice(frameIdx, 1);
    frameIdx = Math.max(0, frameIdx - 1);
    afterEdit();
  };
  $('#f-ms').onchange = (e) => { const a = anim(); ensureDurations(a); a.durations[frameIdx] = Math.max(16, Number(e.target.value) || 150); markDirty(); };
  $('#f-loop').onchange = (e) => { anim().loop = e.target.checked; markDirty(); };

  // ------------------------------------------------------------- meta
  $('#f-name').oninput = (e) => { doc.name = e.target.value; markDirty(); };
  $('#f-id').oninput = (e) => { doc.id = e.target.value.toLowerCase().replace(/[^a-z0-9-_]/g, '-'); markDirty(); };
  $('#f-author').oninput = (e) => { doc.author = e.target.value; markDirty(); };
  $('#f-desc').oninput = (e) => { doc.description = e.target.value; markDirty(); };
  $('#resize').onclick = () => {
    const nw = Math.max(8, Math.min(96, Number($('#f-w').value) || doc.frameWidth));
    const nh = Math.max(8, Math.min(96, Number($('#f-h').value) || doc.frameHeight));
    if (nw === doc.frameWidth && nh === doc.frameHeight) return;
    pushUndo();
    const dw = nw - doc.frameWidth, left = Math.floor(dw / 2);
    for (const a of Object.values(doc.animations)) {
      a.frames = a.frames.map((rows) => {
        const cut = Math.floor(-dw / 2);
        let r = rows.map((line) => (dw >= 0 ? '.'.repeat(left) + line + '.'.repeat(dw - left) : line.slice(cut, cut + nw)));
        r = r.map((line) => line.padEnd(nw, '.').slice(0, nw));
        // keep feet on the ground: add / remove rows at the top
        const dh = nh - r.length;
        if (dh > 0) r = [...Array.from({ length: dh }, () => '.'.repeat(nw)), ...r];
        if (dh < 0) r = r.slice(-dh);
        return r;
      });
    }
    doc.frameWidth = nw;
    doc.frameHeight = nh;
    renderAll();
    markDirty();
  };

  async function save(andUse) {
    if (!doc) return;
    if (curSource === 'builtin' || curSource === 'codex' || curSource === 'petdex') {
      const orig = pets.find((p) => p.key === curKey);
      if (orig && doc.id === orig.id) { doc.id += '-mine'; doc.name = (doc.name || '') + ' (mine)'; fillMeta(doc); }
    }
    try {
      const res = await window.studio.save(doc);
      curKey = res.key;
      curSource = 'user';
      dirty = false;
      document.title = 'Pet Studio — ' + (doc.name || doc.id);
      showWarnings(res.normalized.warnings || []);
      await refreshList();
      if (andUse) await window.studio.use(res.key);
      toast(andUse ? 'Saved. Your pet is on the desktop now.' : 'Saved.');
    } catch (e) {
      toast(e.message);
    }
  }
  $('#save').onclick = () => save(false);
  $('#use').onclick = () => save(true);
  $('#reveal').onclick = () => curKey && window.studio.reveal(curKey);
  $('#new-blank').onclick = newBlank;
  $('#new-copy').onclick = copyCurrent;

  // ------------------------------------------------------------ preview
  let previewTimer = null;
  function schedulePreview() {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(updatePreview, 150);
  }
  async function updatePreview() {
    if (!doc) return;
    const n = normalize();
    await player.setPet(n);
    player.setHeight(Math.min(150, doc.frameHeight * Math.max(1, Math.floor(140 / doc.frameHeight))));
    player.setState(previewState && n.animations[previewState] ? previewState : state);
    buildPreviewStates(Object.keys(n.animations));
  }
  function buildPreviewStates(list) {
    const el = $('#preview-states');
    el.innerHTML = '';
    for (const s of list) {
      const b = document.createElement('button');
      b.textContent = s;
      b.className = s === (previewState || state) ? 'on' : '';
      b.onclick = () => {
        previewState = s;
        if (ONCE.has(s)) player.playOnce(s); else player.setState(s);
        buildPreviewStates(list);
      };
      el.append(b);
    }
  }
  function showWarnings(list) {
    $('#warnings').innerHTML = list.map((w) => `<div>⚠︎ ${escapeHtml(w)}</div>`).join('');
  }

  // ------------------------------------------------------------- render
  function renderAll() {
    if (!doc) return;
    if (!anim()) { state = Object.keys(doc.animations)[0]; frameIdx = 0; }
    frameIdx = Math.min(frameIdx, anim().frames.length - 1);
    previewState = null;
    renderStates();
    renderTools();
    renderPalette();
    fitCanvas();
    drawCanvas();
    renderFrames();
    schedulePreview();
  }

  window.addEventListener('resize', () => { if (doc && !readonly) { fitCanvas(); drawCanvas(); } });

  window.addEventListener('keydown', (e) => {
    if (!document.body.classList.contains('tab-editor')) return;
    const typing = e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT';
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === 's') { e.preventDefault(); save(false); return; }
    if (typing) return;
    if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); e.shiftKey ? doRedo() : doUndo(); return; }
    if (!doc || readonly) return;
    const k = e.key.toLowerCase();
    if (k === 'p' || k === 'b') setTool('pencil');
    else if (k === 'e') setTool('eraser');
    else if (k === 'f' || k === 'g') setTool('fill');
    else if (k === 'i') setTool('picker');
    else if (e.key === 'ArrowRight' || e.key === '.') { frameIdx = (frameIdx + 1) % anim().frames.length; drawCanvas(); renderFrames(); }
    else if (e.key === 'ArrowLeft' || e.key === ',') { frameIdx = (frameIdx - 1 + anim().frames.length) % anim().frames.length; drawCanvas(); renderFrames(); }
  });

  window.studio.onOpen((key) => openPet(key));

  // boot
  (async () => {
    await refreshList();
    const want = window.__INITIAL_PET__ || new URLSearchParams(location.search).get('pet');
    const start = (want && pets.find((p) => p.key === want)) || pets.find((p) => p.key === 'builtin:pip') || pets[0];
    if (start) openPet(start.key); else newBlank();
  })();
})();
