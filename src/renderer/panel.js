/* List window: one card per Claude session, plus toasts. Opens above or below the pet. */
(function () {
  'use strict';

  const $ = (sel) => document.querySelector(sel);
  const rootEl = $('#root');
  const panel = $('#panel');
  const head = $('#panel-head');
  const cardsEl = $('#cards');
  const toast = $('#toast');

  let cfg = {};
  let hookState = 'none';
  let sessions = [];
  let panelOpen = false;
  let showOlder = false;
  let toastTimer = null;
  let toastKey = null;
  let layout = { v: 'above', h: 'right', zoom: 1 };
  let interactive = false;
  let hovering = false;
  let lastMouse = null;
  head.innerHTML = '<span class="count"></span><span class="hustle" hidden>⚡ Hustle mode</span><span class="spacer"></span><span class="more"></span><button class="clear" hidden title="Close every ready and closed session card">Clear finished</button>';
  const headCount = head.querySelector('.count');
  const headMore = head.querySelector('.more');
  const headClear = head.querySelector('.clear');
  headClear.addEventListener('click', (e) => { e.stopPropagation(); window.pet.clearFinished(); });

  const VISIBLE_CARDS = 3;   // show this many cards; scroll for the rest
  const PEEK = 16;           // let the next card peek in so it's obvious there's more
  let scrollToEndNext = false;
  const cardEls = new Map();
  const answerDrafts = new Map(); // question id -> { [question]: Set(labels) }

  // ------------------------------------------------------------------ utils
  const esc = (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

  function ago(ms) {
    const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
    if (s < 60) return `${s}s`;
    const m = Math.round(s / 60);
    if (m < 60) return `${m}m`;
    const h = Math.round(m / 60);
    if (h < 48) return `${h}h`;
    return `${Math.round(h / 24)}d`;
  }

  function hostLabel(s) {
    const e = s.entrypoint || '';
    if (e === 'claude-desktop') return 'Claude app';
    if (/vscode/i.test(e)) return 'VS Code';
    if (/jetbrains/i.test(e)) return 'JetBrains';
    const t = (s.host && s.host.termProgram) || '';
    if (t === 'Apple_Terminal') return 'Terminal';
    if (t === 'iTerm.app') return 'iTerm';
    if (t === 'vscode') return 'VS Code terminal';
    if (t) return t.replace(/\.app$/, '');
    if (e === 'cli') return 'Terminal';
    if (e === 'sdk-ts' || e === 'sdk-py') return 'Agent SDK';
    return '';
  }

  const ICONS = { run: '›_', read: '¶', edit: '✎', search: '⌕', web: '◍', agent: '◆', plan: '☰', ask: '?', skill: '✦', tool: '⚙', think: '…', ok: '✓' };
  const icon = (k) => ICONS[k] || '•';

  function isQuiet(s) {
    return s.state === 'idle' && Date.now() - s.updatedAt > (cfg.quietAfterHours || 12) * 3600e3;
  }

  // ------------------------------------------------- click-through + hover
  function hit(x, y) {
    const el = document.elementFromPoint(x, y);
    if (!el) return false;
    if (panelOpen && panel.contains(el) && el !== panel && el !== cardsEl) return true;
    if (panelOpen && el === cardsEl) return true; // scrollbar / gaps between cards
    if (toast.classList.contains('show') && toast.contains(el)) return true;
    return false;
  }

  function setHover(over) {
    if (over !== interactive) { interactive = over; window.pet.setInteractive(over); }
    if (over !== hovering) { hovering = over; window.pet.hover(over && panelOpen); }
  }

  function refreshInteractive() {
    if (lastMouse) setHover(hit(lastMouse.x, lastMouse.y));
    else setHover(false);
  }

  document.addEventListener('mousemove', (e) => {
    lastMouse = { x: e.clientX, y: e.clientY };
    setHover(hit(e.clientX, e.clientY));
  });
  document.addEventListener('mouseleave', () => { lastMouse = null; setHover(false); });

  // ------------------------------------------------------------ layout
  function applyLayout() {
    const z = layout.zoom || 1;
    document.body.className = `v-${layout.v} h-${layout.h}`;
    rootEl.style.zoom = z;
    rootEl.style.width = window.innerWidth / z + 'px';
    rootEl.style.height = window.innerHeight / z + 'px';
    render();
  }
  window.addEventListener('resize', applyLayout);

  function setOpen(open) {
    if (open && !panelOpen) scrollToEndNext = true;
    panelOpen = open;
    panel.classList.toggle('open', open);
    if (open) hideToast();
    render();
    refreshInteractive();
  }

  // ---------------------------------------------------------------- toast
  function showToast(html, key, ms = 6000) {
    if (panelOpen) return;
    toast.classList.toggle('right', layout.h === 'right');
    toast.innerHTML = html;
    toastKey = key;
    toast.classList.add('show');
    refreshInteractive();
    clearTimeout(toastTimer);
    toastTimer = setTimeout(hideToast, ms);
  }
  function hideToast() {
    toast.classList.remove('show');
    toastKey = null;
    refreshInteractive();
  }
  toast.addEventListener('click', () => {
    if (toastKey) window.pet.focusSession(toastKey);
    hideToast();
  });

  // -------------------------------------------------------------- render
  /** Is the list scrolled to the end next to the pet? */
  function atPetEnd() {
    if (layout.v === 'above') return cardsEl.scrollHeight - cardsEl.scrollTop - cardsEl.clientHeight < 4;
    return cardsEl.scrollTop < 4;
  }

  function scrollToPetEnd() {
    cardsEl.scrollTop = layout.v === 'above' ? cardsEl.scrollHeight : 0;
  }

  /** Size the list to exactly the 3 cards nearest the pet (plus a peek of the next one). */
  function fitToCards() {
    const items = [...cardsEl.children];
    const extra = items.length - VISIBLE_CARDS;
    cardsEl.classList.toggle('scrolls', extra > 0);
    if (extra <= 0) { cardsEl.style.maxHeight = ''; return extra; }
    const nearest = layout.v === 'above' ? items.slice(-VISIBLE_CARDS) : items.slice(0, VISIBLE_CARDS);
    const h = nearest.reduce((sum, el) => sum + el.offsetHeight, 0) + 6 * VISIBLE_CARDS + 6 + PEEK;
    cardsEl.style.maxHeight = h + 'px';
    return extra;
  }

  function updateFade() {
    const far = layout.v === 'above'
      ? cardsEl.scrollTop > 2
      : cardsEl.scrollHeight - cardsEl.scrollTop - cardsEl.clientHeight > 2;
    cardsEl.classList.toggle('more-far', far);
  }
  cardsEl.addEventListener('scroll', updateFade, { passive: true });

  function render() {
    if (!panelOpen) return;
    // If you were at the pet end, stay there; if you scrolled away, Chromium's scroll anchoring
    // keeps what you're looking at in place while cards above or below it change.
    const stick = scrollToEndNext || atPetEnd();
    scrollToEndNext = false;

    const visible = cfg.showQuietSessions ? sessions : sessions.filter((s) => s.state !== 'idle');
    const active = visible.filter((s) => !isQuiet(s));
    const quiet = visible.filter((s) => isQuiet(s));

    const need = sessions.filter((s) => s.state === 'question').length;
    const working = sessions.filter((s) => s.state === 'working').length;
    const finished = sessions.filter((s) => s.state === 'done' || s.state === 'ended').length;
    const bits = [`${sessions.length} session${sessions.length === 1 ? '' : 's'}`];
    if (working) bits.push(`${working} working`);
    if (need) bits.push(`${need} need${need === 1 ? 's' : ''} you`);
    headCount.textContent = bits.join(' · ');
    headClear.hidden = finished === 0;
    headClear.textContent = `Clear ${finished} finished`;

    const want = [];
    if (!sessions.length) want.push({ empty: true });
    for (const s of active) want.push(s);
    if (quiet.length) {
      want.push({ older: quiet.length });
      if (showOlder) for (const s of quiet) want.push({ ...s, _quiet: true });
    }
    if (hookState === 'none' && !cfg.demo) want.push({ banner: 'hooks' });

    // Keyed update so buttons don't get swapped out from under the cursor.
    const keep = new Set();
    const order = [];
    for (const item of want) {
      let key, el;
      if (item.banner) {
        key = '__banner';
        el = cardEls.get(key) || makeBanner();
      } else if (item.empty) {
        key = '__empty';
        el = cardEls.get(key) || Object.assign(document.createElement('div'), { className: 'empty', textContent: 'No Claude Code sessions running. Start one and I’ll keep an eye on it.' });
      } else if (item.older) {
        key = '__older';
        el = cardEls.get(key) || document.createElement('div');
        el.className = 'older';
        el.textContent = `${showOlder ? '▾' : '▸'} ${item.older} quiet session${item.older === 1 ? '' : 's'}`;
        el.onclick = () => { showOlder = !showOlder; render(); };
      } else {
        key = item.key;
        el = cardEls.get(key) || makeCard(item.key);
        updateCard(el, item);
      }
      cardEls.set(key, el);
      keep.add(key);
      order.push(el);
    }
    for (const [k, el] of cardEls) if (!keep.has(k)) { el.remove(); cardEls.delete(k); }
    // Most urgent card sits next to the pet.
    const seq = layout.v === 'above' ? [...order].reverse() : order;
    seq.forEach((el, i) => { if (cardsEl.children[i] !== el) cardsEl.insertBefore(el, cardsEl.children[i] || null); });
    const more = fitToCards();
    if (stick) scrollToPetEnd();
    updateFade();
    headMore.textContent = more > 0 ? `${layout.v === 'above' ? '↑' : '↓'} ${more} more` : '';
  }

  function makeBanner() {
    const el = document.createElement('div');
    el.className = 'banner';
    el.innerHTML = `Watching sessions from Claude's status files. <b>Connect live hooks</b> to see every tool call and subagent.
      <div class="sub">With answering on, permission prompts and Claude's questions show up here with buttons. The pet only holds a prompt while you're in a different app, and hands it back to Claude if you don't answer.</div>
      <div class="btns"><button class="primary" data-a="1">Connect + answering</button><button data-a="0">Status only</button><button class="ghost">Not now</button></div>`;
    const [full, status, no] = el.querySelectorAll('button');
    full.onclick = (e) => { e.stopPropagation(); window.pet.connectHooks(true); };
    status.onclick = (e) => { e.stopPropagation(); window.pet.connectHooks(false); };
    no.onclick = (e) => { e.stopPropagation(); hookState = 'dismissed'; render(); };
    return el;
  }

  const WHY = {
    pretend: 'Pretend session from the simulator: there’s no window to open.',
    closed: 'This session is closed.',
    gone: 'This session is gone.',
    unknown: 'Couldn’t find the window for this session.',
  };

  /** Open a session; if that can't happen, shake the card and say why. */
  async function openFrom(el, key) {
    const res = await window.pet.focusSession(key);
    if (!res || res.ok) return;
    el.classList.remove('nope');
    void el.offsetWidth;
    el.classList.add('nope');
    let note = el.querySelector('.why');
    if (!note) {
      note = document.createElement('div');
      note.className = 'why';
      el.append(note);
    }
    note.textContent = WHY[res.reason] || WHY.unknown;
    clearTimeout(note._t);
    note._t = setTimeout(() => note.remove(), 2600);
  }

  function makeCard(key) {
    const el = document.createElement('div');
    el.className = 'card';
    el.dataset.key = key;
    el.innerHTML = '<button class="x" title="Close this card">×</button><div class="info"></div><div class="subslot"></div><div class="tail"></div><div class="qslot"></div>';
    el.querySelector('.x').addEventListener('click', (e) => {
      e.stopPropagation();
      window.pet.dismiss(key);
    });
    el.addEventListener('click', (e) => {
      if (e.target.closest('.q') || e.target.closest('.subs') || e.target.closest('.x')) return;
      openFrom(el, key);
    });
    // Hover a card to unfold every subagent under it.
    const fold = (open) => {
      const stick = atPetEnd();
      el._hover = open;
      el.classList.toggle('expanded', open);
      fitToCards();
      if (stick) scrollToPetEnd();
      if (open) el.scrollIntoView({ block: 'nearest' }); // show the whole unfolded card
      updateFade();
    };
    el.addEventListener('mouseenter', () => fold(true));
    el.addEventListener('mouseleave', () => fold(false));
    return el;
  }

  function updateCard(el, s) {
    const kind = s.state === 'done' && s.attention && s.attention.kind === 'error' ? 'error' : s.state;
    const closable = kind === 'done' || kind === 'error' || kind === 'idle' || kind === 'ended' || (kind === 'question' && !s.question && s.attention && s.attention.ask);
    el.className = `card ${kind}${s._quiet ? ' quiet' : ''}${closable ? ' closable' : ''}${el._hover ? ' expanded' : ''}`;
    const info = el.querySelector('.info');

    let time = '';
    if (s.state === 'working' && s.workingSince) time = ago(s.workingSince);
    else if (s.state === 'ended' && s.endedAt) time = ago(s.endedAt);
    else if (s.attention) time = ago(s.attention.at);
    else time = ago(s.updatedAt);

    const host = hostLabel(s);
    const projectLine = [s.project && s.project !== s.title ? s.project : '', host].filter(Boolean).join(' · ');

    let html = `<div class="row1"><span class="dot"></span><span class="title">${esc(s.title)}</span><span class="time">${esc(time)}</span></div>`;
    if (projectLine) html += `<div class="row2">${esc(projectLine)}</div>`;

    if (s.plan && (s.state === 'working' || s.state === 'question')) {
      const pct = Math.round(((s.plan.step - 1) / s.plan.total) * 100);
      html += `<div class="plan"><span class="txt">Step ${s.plan.step}/${s.plan.total} · ${esc(s.plan.text)}</span><div class="bar"><div style="width:${pct}%"></div></div></div>`;
    }
    if (s.state === 'working' || (s.state === 'question' && !s.question)) {
      if (s.activity) html += `<div class="activity"><span class="ico">${esc(icon(s.activity.icon))}</span><span class="txt">${esc(s.activity.text)}</span></div>`;
      else if (s.state === 'working') html += `<div class="activity"><span class="ico">…</span><span class="txt">Working</span></div>`;
    }
    if (info.innerHTML !== html) info.innerHTML = html;

    // Subagents: one line folded, all of them (scrollable) when you hover the card.
    const subslot = el.querySelector('.subslot');
    let subs = '';
    if (s.subagents && s.subagents.length) {
      const n = s.subagents.length;
      subs += `<div class="subs-head"><span class="ico">◆</span><span class="txt">${n} subagent${n === 1 ? '' : 's'} running</span>${n > 1 ? '<span class="hint">hover to see all</span>' : ''}</div><div class="subs">`;
      for (const a of s.subagents) {
        subs += `<div class="sub"><span class="spin"></span><span class="kind">${esc(a.type)}</span><span class="txt">${esc(a.description)}${a.activity ? ' — ' + esc(a.activity) : ''}</span></div>`;
      }
      subs += '</div>';
    }
    if (subslot._html !== subs) {
      const list = subslot.querySelector('.subs');
      const keepScroll = list ? list.scrollTop : 0;
      subslot.innerHTML = subs;
      subslot._html = subs;
      const next = subslot.querySelector('.subs');
      if (next) next.scrollTop = keepScroll;
    }

    html = '';
    if (s.state === 'question' && !s.question && s.attention) {
      html += s.attention.ask
        ? `<div class="note ask"><b>Claude is asking:</b> ${esc(s.attention.text)} <span class="open">Click to answer.</span></div>`
        : `<div class="note">${esc(s.attention.text || 'Waiting for you')} — click to open.</div>`;
    }
    if (s.state === 'done' && s.attention && s.attention.text) {
      html += `<div class="note">${esc(s.attention.text)}</div>`;
    }
    if (s.state === 'idle' && s.lastPrompt && !s._quiet) {
      html += `<div class="note">Last: ${esc(s.lastPrompt)}</div>`;
    }
    if (s.stale) {
      html += `<div class="note">No news from this session in a while. It might be waiting on you. Click to check.</div>`;
    }
    if (s.state === 'ended') {
      html += `<div class="note">Session closed. Press × to clear it.</div>`;
    }
    const tail = el.querySelector('.tail');
    if (tail.innerHTML !== html) tail.innerHTML = html;

    // Question block: rebuilt only when the question itself changes.
    const slot = el.querySelector('.qslot');
    const sig = s.question ? `${s.question.id}:${s.question.answerable}` : '';
    if (!slot._leaving && slot.dataset.sig !== sig) {
      slot.dataset.sig = sig;
      slot.innerHTML = '';
      if (s.question) slot.append(buildQuestion(s));
    }
    const bar = slot.querySelector('.timer > div');
    if (bar && s.question && s.question.expiresAt) {
      const total = s.question.expiresAt - (s.question.createdAt || s.question.expiresAt - (cfg.holdSeconds || 45) * 1000);
      const left = Math.max(0, s.question.expiresAt - Date.now());
      bar.style.transform = `scaleX(${Math.min(1, left / Math.max(1, total))})`;
      const count = slot.querySelector('.count');
      if (count) count.textContent = `${Math.ceil(left / 1000)}s`;
    }
  }

  /**
   * You answered from the pet: the button you clicked pops in a little pixel burst
   * (green for Allow, red for Deny, yellow for a picked option), then the whole prompt swipes off
   * to the right and the card closes up. The answer itself is sent right away.
   */
  function popAndSwipe(box, btn, color) {
    const slot = box.parentElement;
    if (!slot || slot._leaving) return;
    slot._leaving = true;
    box.classList.add('answered');
    if (btn) {
      const burst = document.createElement('div');
      burst.className = 'burst';
      burst.style.left = btn.offsetLeft + btn.offsetWidth / 2 + 'px';
      burst.style.top = btn.offsetTop + btn.offsetHeight / 2 + 'px';
      burst.style.setProperty('--c', color);
      for (let i = 0; i < 14; i++) {
        const p = document.createElement('i');
        const a = (i / 14) * Math.PI * 2 + Math.random() * 0.35;
        const d = 22 + Math.random() * 26;
        p.style.setProperty('--dx', Math.round(Math.cos(a) * d) + 'px');
        p.style.setProperty('--dy', Math.round(Math.sin(a) * d) + 'px');
        burst.append(p);
      }
      box.append(burst);
      btn.classList.add('pop');
    }
    setTimeout(() => box.classList.add('swipe'), 280);
    setTimeout(() => {
      box.style.height = box.offsetHeight + 'px';
      requestAnimationFrame(() => box.classList.add('collapse'));
    }, 600);
    setTimeout(() => {
      slot._leaving = false;
      slot.dataset.sig = '__answered';
      render();
    }, 860);
  }

  function buildQuestion(s) {
    const q = s.question;
    const box = document.createElement('div');
    box.className = 'q';
    box.addEventListener('click', (e) => e.stopPropagation());

    const open = () => { const card = box.closest('.card'); if (card) openFrom(card, s.key); else window.pet.focusSession(s.key); };

    if (q.kind === 'permission') {
      box.innerHTML = `<div class="qhead">${esc(q.title)}</div>${q.body ? (q.mono ? `<pre>${esc(q.body)}</pre>` : `<div class="body">${esc(q.body)}</div>`) : ''}${q.note ? `<div class="later">${esc(q.note)}</div>` : ''}`;
      const btns = document.createElement('div');
      btns.className = 'btns';
      if (q.answerable) {
        for (const o of q.options || []) {
          const b = document.createElement('button');
          b.className = o.style || '';
          b.textContent = o.label;
          b.onclick = () => {
            window.pet.answer(q.id, { choice: o.id });
            popAndSwipe(box, b, o.id === 'deny' ? 'var(--error)' : 'var(--done)');
          };
          btns.append(b);
        }
      }
      const ob = document.createElement('button');
      ob.className = q.answerable ? 'ghost' : '';
      ob.textContent = q.answerable ? 'Open ↗' : 'Answer in Claude ↗';
      ob.onclick = open;
      btns.append(ob);
      box.append(btns);
    } else if (q.kind === 'ask') {
      const draft = answerDrafts.get(q.id) || {};
      answerDrafts.set(q.id, draft);
      const multi = q.questions.length > 1 || q.questions.some((x) => x.multiSelect);
      for (const item of q.questions) {
        const h = document.createElement('div');
        h.className = 'qhead';
        h.innerHTML = `${item.header ? `<span class="chip">${esc(item.header)}</span>` : ''}<span>${esc(item.question)}</span>`;
        box.append(h);
        const opts = document.createElement('div');
        opts.className = 'opts';
        for (const o of item.options) {
          const b = document.createElement('button');
          b.className = 'opt';
          b.innerHTML = `${esc(o.label)}${o.description ? `<small>${esc(o.description)}</small>` : ''}`;
          b.disabled = !q.answerable;
          const sel = () => draft[item.question] || (draft[item.question] = new Set());
          if (sel().has(o.label)) b.classList.add('on');
          b.onclick = () => {
            if (!multi) {
              window.pet.answer(q.id, { answers: { [item.question]: o.label } });
              popAndSwipe(box, b, 'var(--question)');
              return;
            }
            const set = sel();
            if (!item.multiSelect) set.clear();
            if (set.has(o.label)) set.delete(o.label); else set.add(o.label);
            for (const btn of opts.children) btn.classList.toggle('on', set.has(btn.firstChild.textContent));
            submit.disabled = !q.questions.every((x) => draft[x.question] && draft[x.question].size);
          };
          opts.append(b);
        }
        box.append(opts);
      }
      const btns = document.createElement('div');
      btns.className = 'btns';
      const submit = document.createElement('button');
      if (q.answerable && multi) {
        submit.className = 'primary';
        submit.textContent = 'Send';
        submit.disabled = true;
        submit.onclick = () => {
          const answers = {};
          for (const x of q.questions) answers[x.question] = [...(draft[x.question] || [])].join(', ');
          window.pet.answer(q.id, { answers });
          popAndSwipe(box, submit, 'var(--done)');
        };
        btns.append(submit);
      }
      const ob = document.createElement('button');
      ob.className = q.answerable ? 'ghost' : '';
      ob.textContent = q.answerable ? 'Type my own in Claude ↗' : 'Answer in Claude ↗';
      ob.onclick = open;
      btns.append(ob);
      box.append(btns);
    }

    if (q.answerable && q.expiresAt) {
      const count = document.createElement('span');
      count.className = 'count';
      count.title = 'If you don’t answer here, Claude asks in its own window when this runs out.';
      box.querySelector('.btns').append(count);
      const t = document.createElement('div');
      t.className = 'timer';
      t.innerHTML = '<div></div>';
      box.append(t);
    }
    return box;
  }

  // ------------------------------------------------------------- wiring
  window.pet.on('init', (init) => {
    cfg = { ...init.config, demo: init.demo };
    if (hookState !== 'dismissed' || init.hooks !== 'none') hookState = init.hooks;
    sessions = init.sessions || [];
    render();
  });
  window.pet.on('layout', (l) => { layout = l; applyLayout(); });
  window.pet.on('sessions', (list) => { sessions = list; render(); });
  window.pet.on('panel-state', ({ open }) => setOpen(open));
  window.pet.on('hide-toast', hideToast);
  let hustleEnds = 0;
  window.pet.on('hustle', ({ until }) => { hustleEnds = until; tickHustle(); });
  function tickHustle() {
    const chip = head.querySelector('.hustle');
    const left = Math.ceil((hustleEnds - Date.now()) / 1000);
    chip.hidden = left <= 0;
    if (left > 0) { chip.textContent = `⚡ Hustle mode ${left}s`; setTimeout(tickHustle, 500); }
  }
  window.pet.on('attention', (s) => {
    if (s.state === 'question' || (s.attention && s.attention.kind === 'question')) {
      const what = s.question ? s.question.title : (s.attention && s.attention.text) || 'Needs your input';
      showToast(`<b>${esc(s.title)}</b><span class="tsub">${esc(what)}</span>`, s.key, 9000);
    } else if (s.attention && s.attention.kind === 'error') {
      showToast(`<b>${esc(s.title)}</b><span class="tsub">Stopped: ${esc(s.attention.text)}</span>`, s.key);
    } else if (s.attention && s.attention.kind === 'done') {
      showToast(`<b>${esc(s.title)}</b><span class="tsub">✓ ${esc(s.attention.text || 'Ready for you')}</span>`, s.key, 5000);
    }
  });

  // Relative times + hold countdowns.
  setInterval(() => { if (panelOpen) render(); }, 1000);

  applyLayout();
  window.pet.ready();
})();
