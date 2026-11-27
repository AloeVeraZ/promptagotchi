/*
 * The bridge between these pages and Promptagotchi's Rust core (Tauri).
 *
 * Every page talks through three small objects: window.pet (pet, list and whip windows),
 * window.control (the app window's tabs) and window.studio (the Editor). Each call is one
 * `ipc` command with a channel name; events from Rust arrive through `on(name, fn)`.
 *
 * Click-through: the overlay windows ignore the mouse except over the pet or the cards. This
 * reports where those are, and the Rust side flips click-through on and off as the cursor moves.
 */
(function () {
  'use strict';
  const T = window.__TAURI__;
  if (!T) return;

  const invoke = (channel, payload) => T.core.invoke('ipc', { channel, payload: payload === undefined ? null : payload });
  const send = (channel, payload) => { invoke(channel, payload).catch(() => {}); };
  const on = (name, fn) => {
    const unlisten = T.event.listen(name, (e) => fn(e.payload));
    return () => unlisten.then((u) => u());
  };

  window.pet = {
    on,
    ready: () => send('ui:ready'),
    // Hover and click-through are handled by the Rust side from the cursor position.
    setInteractive: () => {},
    hover: () => {},
    drag: (msg) => send('pet:drag', msg),
    click: () => send('pet:click'),
    openMenu: () => send('pet:menu'),
    reportContent: (c) => send('pet:content', c),
    whipHit: (h) => send('whip:hit', h),
    focusSession: (key) => invoke('session:focus', key),
    markSeen: (key) => send('session:seen', key),
    dismiss: (key) => send('session:dismiss', key),
    clearFinished: () => send('sessions:clear-finished'),
    answer: (id, answer) => send('question:answer', { id, answer }),
    release: (id) => send('question:release', id),
    connectHooks: (answer) => send('hooks:connect', !!answer),
    openStudio: () => send('studio:open'),
  };

  window.control = {
    get: () => invoke('control:get'),
    act: (name, arg) => invoke('control:act', { name, arg: arg === undefined ? null : arg }),
    on,
  };

  window.studio = {
    list: () => invoke('studio:list'),
    read: (key) => invoke('studio:read', key),
    normalize: (key) => invoke('studio:normalize', key),
    save: (json) => invoke('studio:save', json),
    use: (key) => invoke('studio:use', key),
    reveal: (key) => invoke('studio:reveal', key),
    onOpen: (fn) => on('studio:open', fn),
  };

  // Where the clickable parts are, in window coordinates.
  const page = location.pathname.split('/').pop();
  const HOT = { 'pet.html': ['#pet'], 'panel.html': ['#panel.open', '#toast.show'] }[page];
  if (!HOT) return;

  // Hover. WebKit only follows the mouse in the focused window, and these overlays never take
  // focus (so they don't steal your typing). The Rust side sends the cursor position as `ptr`
  // while it's over the pet or the cards, and this turns it into mouseenter / mouseleave events
  // and hover styles: every `:hover` rule also matches `[data-hover]` (an attribute, so card
  // updates that rewrite `className` don't wipe it).
  const addHoverRules = () => {
    for (const sheet of document.styleSheets) {
      let rules;
      try { rules = sheet.cssRules; } catch { continue; }
      for (let i = rules.length - 1; i >= 0; i--) {
        const r = rules[i];
        if (!r.selectorText || !r.selectorText.includes(':hover')) continue;
        try { sheet.insertRule(`${r.selectorText.replace(/:hover/g, '[data-hover]')} { ${r.style.cssText} }`, i + 1); } catch {}
      }
    }
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', addHoverRules);
  else addHoverRules();

  let chain = [];
  const mouse = (type, p, bubbles = false) => new MouseEvent(type, { bubbles, clientX: p ? p.x : 0, clientY: p ? p.y : 0 });
  const hoverAt = (p) => {
    const top = p ? document.elementFromPoint(p.x, p.y) : null;
    const next = [];
    for (let el = top; el && el !== document.documentElement; el = el.parentElement) next.push(el);
    for (const el of chain) {
      if (next.includes(el)) continue;
      el.removeAttribute('data-hover');
      if (el.isConnected) el.dispatchEvent(mouse('mouseleave', p));
    }
    for (const el of next.slice().reverse()) {
      if (chain.includes(el)) continue;
      el.setAttribute('data-hover', '');
      el.dispatchEvent(mouse('mouseenter', p));
    }
    chain = next;
    document.dispatchEvent(p ? mouse('mousemove', p, true) : mouse('mouseleave', null));
  };
  let lastPtr = null;
  on('ptr', (p) => { lastPtr = p; hoverAt(p); });
  // Cards come and go under a still cursor: keep the hover right without waiting for a move.
  setInterval(() => { if (lastPtr) hoverAt(lastPtr); }, 250);

  let last = '';
  const report = () => {
    const rects = [];
    for (const sel of HOT) {
      const el = document.querySelector(sel);
      if (!el) continue;
      let r = el.getBoundingClientRect();
      // The list is drawn with CSS zoom on bigger screens; make sure we report what you see.
      const root = el.closest('#root');
      const z = root ? parseFloat(root.style.zoom) || 1 : 1;
      const visualW = el.offsetWidth * z;
      const f = z !== 1 && r.width && Math.abs(r.width - visualW) > 2 ? visualW / r.width : 1;
      if (r.width > 0 && r.height > 0) rects.push([r.left * f, r.top * f, r.width * f, r.height * f].map((n) => Math.round(n)));
    }
    const sig = JSON.stringify(rects);
    if (sig !== last) {
      last = sig;
      send('ui:hot', rects);
    }
  };
  setInterval(report, 100);
  window.addEventListener('resize', report);
})();
