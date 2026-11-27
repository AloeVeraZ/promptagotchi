/* Pet window: the critter, its bubble and badge. Drag it anywhere; hover for the session list. */
(function () {
  'use strict';

  const $ = (s) => document.querySelector(s);
  const petEl = $('#pet');
  const bubble = $('#bubble');
  const badge = $('#badge');
  const root = document.documentElement.style;
  const player = new PetPlayer(petEl);

  let cfg = {};
  let sessions = [];
  let metrics = null;
  let interactive = false;
  let hovering = false;
  let dragging = false;
  let down = null;
  let lastActive = Date.now();
  let nextFidget = Date.now() + 8000;
  const FIDGETS = ['look', 'stretch', 'wave', 'look'];

  // ------------------------------------------------------- hover + click-through
  function setHover(over) {
    if (over !== interactive) { interactive = over; window.pet.setInteractive(over); }
    if (over !== hovering) { hovering = over; window.pet.hover(over); }
  }
  document.addEventListener('mousemove', (e) => {
    if (dragging) return;
    const el = document.elementFromPoint(e.clientX, e.clientY);
    setHover(!!el && petEl.contains(el));
  });
  document.addEventListener('mouseleave', () => { if (!dragging) setHover(false); });

  // -------------------------------------------------------------- drag + click
  // A drag only lives while the left button is really held down. Right-click / control-click
  // open the menu and never start one, and anything odd (the menu swallowing the mouse-up,
  // losing pointer capture, the window losing focus) ends the drag instead of leaving the pet
  // glued to the cursor.
  function endDrag(e, { click = false } = {}) {
    if (!down) return;
    const wasDrag = down.moved;
    const id = down.pointerId;
    down = null;
    try { if (petEl.hasPointerCapture(id)) petEl.releasePointerCapture(id); } catch {}
    if (wasDrag) {
      dragging = false;
      petEl.classList.remove('dragging');
      window.pet.drag({ phase: 'end', screenX: e ? e.screenX : 0, screenY: e ? e.screenY : 0 });
      update();
    } else if (click) {
      window.pet.click();
    }
  }

  petEl.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || e.ctrlKey) return; // right-click / control-click = menu, not drag
    endDrag(null);
    petEl.setPointerCapture(e.pointerId);
    down = { x: e.screenX, y: e.screenY, lastX: e.screenX, moved: false, pointerId: e.pointerId };
  });

  petEl.addEventListener('pointermove', (e) => {
    if (!down) return;
    if ((e.buttons & 1) === 0) return endDrag(e); // button is no longer held: we missed the mouse-up
    if (!down.moved && Math.hypot(e.screenX - down.x, e.screenY - down.y) > 4) {
      down.moved = true;
      dragging = true;
      petEl.classList.add('dragging');
      window.pet.drag({ phase: 'start', screenX: down.x, screenY: down.y });
    }
    if (!down.moved) return;
    window.pet.drag({ phase: 'move', screenX: e.screenX, screenY: e.screenY });
    const vx = e.screenX - down.lastX;
    down.lastX = e.screenX;
    if (player.has('dragLeft') || player.has('dragRight')) {
      if (Math.abs(vx) > 1) player.setState(vx < 0 ? 'dragLeft' : 'dragRight');
    } else {
      player.setState('drag');
    }
  });

  petEl.addEventListener('pointerup', (e) => endDrag(e, { click: e.button === 0 && !e.ctrlKey }));
  petEl.addEventListener('pointercancel', (e) => endDrag(e));
  petEl.addEventListener('lostpointercapture', (e) => endDrag(e));
  window.addEventListener('blur', () => endDrag(null));

  petEl.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    endDrag(e);
    window.pet.openMenu();
  });

  // ------------------------------------------------------------------- sound
  let audio = null;
  function chirp() {
    if (!cfg.sound) return;
    try {
      audio = audio || new AudioContext();
      const t = audio.currentTime;
      [660, 880].forEach((f, i) => {
        const o = audio.createOscillator();
        const g = audio.createGain();
        o.type = 'square';
        o.frequency.value = f;
        g.gain.setValueAtTime(0.0001, t + i * 0.09);
        g.gain.exponentialRampToValueAtTime(0.05, t + i * 0.09 + 0.01);
        g.gain.exponentialRampToValueAtTime(0.0001, t + i * 0.09 + 0.08);
        o.connect(g).connect(audio.destination);
        o.start(t + i * 0.09);
        o.stop(t + i * 0.09 + 0.1);
      });
    } catch {}
  }

  // ------------------------------------------------------------- pet state
  function update() {
    const q = sessions.filter((s) => s.state === 'question');
    const w = sessions.filter((s) => s.state === 'working' && !s.stale);
    const d = sessions.filter((s) => s.state === 'done');
    const err = d.some((s) => s.attention && s.attention.kind === 'error');
    if (q.length || w.length || d.length) lastActive = Date.now();
    else lastActive = Math.max(lastActive, ...sessions.map((s) => s.updatedAt || 0));
    const sleepy = !q.length && !w.length && !d.length && Date.now() - lastActive > 5 * 60 * 1000;

    // Typing only while something is actually working. Finished and waiting for you = standing
    // around with the ✓ bubble, fidgeting now and then.
    let base = 'idle';
    if (q.length) base = 'question';
    else if (w.length) base = 'working';
    else if (err) base = 'error';
    else if (sleepy) base = 'sleep';
    if (!dragging) player.setState(base);

    if (base === 'idle' && !dragging && !player.onceThen && Date.now() > nextFidget) {
      const options = FIDGETS.filter((f) => player.has(f));
      if (options.length) player.playOnce(options[Math.floor(Math.random() * options.length)]);
      // Fidget a bit more often when something is ready and waiting for you.
      nextFidget = Date.now() + (d.length ? 5000 + Math.random() * 5000 : 8000 + Math.random() * 9000);
    }

    bubble.className = '';
    if (q.length) { bubble.textContent = '?'; bubble.className = 'show question'; }
    else if (d.length) { bubble.textContent = err ? '!' : '✓'; bubble.className = 'show ' + (err ? 'error' : 'done'); }
    else if (base === 'sleep') { bubble.textContent = 'z z'; bubble.className = 'show sleep'; }
    else if (w.length > 1) { bubble.textContent = String(w.length); bubble.className = 'show count'; }

    const need = q.length + d.length;
    badge.textContent = String(need);
    badge.classList.toggle('show', need > 1);
    badge.style.background = q.length ? 'var(--question)' : 'var(--done)';
  }

  function applyMetrics() {
    if (!metrics || !player.pet) return;
    player.setHeight(metrics.petH);
    const { w, h } = player.displaySize;
    root.setProperty('--z', metrics.zoom);
    root.setProperty('--pet-h', h + 'px');
    root.setProperty('--pet-w', w + 'px');
    root.setProperty('--pad-bottom', metrics.pad.bottom + 'px');
    // Empty canvas under the feet hangs below the window so the pet stands right on the ground.
    root.setProperty('--sprite-bottom', metrics.pad.bottom - (metrics.clipBottom || 0) + 'px');
    root.setProperty('--head-inset', Math.round((player.headInset || 0) * h) + 'px');
    petEl.classList.toggle('smooth', !player.pet.pixelArt);
  }

  // Image pets (GIFs, sprite sheets, Codex pets): measure where the body is so they get sized
  // like every other pet. Text-pixel pets are measured by the main process already.
  function reportContent() {
    if (!player.pet || player.pet.content) return;
    const c = player.measureContent();
    if (c) window.pet.reportContent({ key: player.pet.key, top: c.top, bottom: c.bottom });
  }

  // ------------------------------------------------------------------ wiring
  window.pet.on('init', async (init) => {
    cfg = init.config || {};
    sessions = init.sessions || [];
    if (init.pet && (!player.pet || player.pet.key !== init.pet.key || JSON.stringify(player.pet.animations) !== JSON.stringify(init.pet.animations))) {
      await player.setPet(init.pet);
      reportContent();
    }
    applyMetrics();
    update();
  });
  window.pet.on('metrics', (m) => { metrics = m; applyMetrics(); });
  window.pet.on('sessions', (list) => { sessions = list; update(); });
  window.pet.on('attention', (s) => {
    if (s.state === 'question' || (s.attention && s.attention.kind === 'question')) {
      player.playOnce('jump');
      chirp();
    } else if (s.attention && s.attention.kind === 'done') {
      // A little celebration hop, unless other sessions are still busy (then keep typing).
      if (!sessions.some((x) => x.state === 'working' || x.state === 'question')) {
        player.playOnce('done');
        nextFidget = Date.now() + 6000;
      }
    }
  });
  // ------------------------------------------------------- whip + hustle
  let hustleTimer = null;
  window.pet.on('whip-mode', ({ on }) => document.body.classList.toggle('nervous', !!on));
  window.pet.on('pet:hit', () => {
    player.playOnce(player.has('hit') ? 'hit' : 'jump');
    petEl.classList.remove('ouch');
    void petEl.offsetWidth;
    petEl.classList.add('ouch');
    bubble.textContent = '!!';
    bubble.className = 'show error';
    setTimeout(update, 900);
  });
  window.pet.on('hustle', ({ until }) => {
    const left = until - Date.now();
    if (left <= 0) return;
    player.speed = 2.2;
    document.body.classList.add('hustle');
    clearTimeout(hustleTimer);
    hustleTimer = setTimeout(() => { player.speed = 1; document.body.classList.remove('hustle'); }, left);
  });

  window.pet.on('panel-state', ({ open }) => {
    if (open && (player.base === 'idle' || player.base === 'sleep')) player.playOnce('wave');
  });

  setInterval(update, 1000);
  window.pet.ready();
})();
