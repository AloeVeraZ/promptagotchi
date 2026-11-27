/* The app window: tab switching, and the Run tab (pet, Claude Code, sessions, debug, live events). */
(function () {
  'use strict';

  const $ = (s) => document.querySelector(s);
  const esc = (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const ago = (ms) => {
    if (!ms) return '';
    const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
    if (s < 60) return `${s}s ago`;
    const m = Math.round(s / 60);
    if (m < 60) return `${m}m ago`;
    const h = Math.round(m / 60);
    return h < 48 ? `${h}h ago` : `${Math.round(h / 24)}d ago`;
  };

  // ------------------------------------------------------------------- tabs
  const TABS = ['home', 'editor', 'settings', 'debug'];
  function showTab(tab) {
    if (!TABS.includes(tab)) tab = 'home';
    for (const t of TABS) document.body.classList.toggle('tab-' + t, t === tab);
    document.querySelectorAll('#tabs .tab').forEach((b) => b.classList.toggle('on', b.dataset.tab === tab));
    // Pet Studio sizes its canvas to the space it has; it had none while hidden.
    if (tab === 'editor') requestAnimationFrame(() => window.dispatchEvent(new Event('resize')));
  }
  document.querySelectorAll('#tabs .tab').forEach((b) => b.addEventListener('click', () => showTab(b.dataset.tab)));
  window.addEventListener('keydown', (e) => {
    if (!(e.metaKey || e.ctrlKey)) return;
    const i = ['1', '2', '3', '4'].indexOf(e.key);
    if (i >= 0) { e.preventDefault(); showTab(TABS[i]); }
  });
  window.control.on('app:tab', (tab) => showTab(tab));
  showTab(window.__INITIAL_TAB__ || new URLSearchParams(location.search).get('tab') || 'home');

  // --------------------------------------------------------------- the pet
  const player = new PetPlayer($('#home-pet'));
  let state = null;

  function petState(sessions) {
    if (sessions.some((s) => s.state === 'question')) return 'question';
    if (sessions.some((s) => s.state === 'working')) return 'working';
    if (sessions.some((s) => s.state === 'done' && s.attention && s.attention.kind === 'error')) return 'error';
    return 'idle';
  }

  let fidgetAt = Date.now() + 5000;
  setInterval(() => {
    if (player.base === 'idle' && !player.onceThen && Date.now() > fidgetAt) {
      const f = ['look', 'stretch', 'wave'].filter((x) => player.has(x));
      if (f.length) player.playOnce(f[Math.floor(Math.random() * f.length)]);
      fidgetAt = Date.now() + 6000 + Math.random() * 6000;
    }
  }, 1000);

  async function renderPet() {
    const pet = state.pet;
    if (pet && (!player.pet || player.pet.key !== pet.key || JSON.stringify(player.pet.animations) !== JSON.stringify(pet.animations))) {
      await player.setPet(pet);
      const c = pet.content;
      const bodyRows = c ? c.bottom - c.top + 1 : pet.frameHeight;
      // Same rule as the desktop: size by the visible body.
      player.setHeight(Math.round(pet.frameHeight * (pet.pixelArt === false ? 92 / bodyRows : Math.max(1, Math.round(92 / bodyRows)))));
    }
    player.setState(petState(state.sessions));
    $('#pet-name').textContent = pet ? pet.name : 'Promptagotchi';
    const st = $('#pet-status');
    st.textContent = state.hidden ? 'Hidden. Bring it back with Show pet or ⌥⇧P' : 'On your screen, watching your Claude sessions';
    st.classList.toggle('off', state.hidden);
    $('#btn-toggle').textContent = state.hidden ? 'Show pet' : 'Hide pet';

    const sel = $('#sel-pet');
    const sig = state.pets.map((p) => p.key).join('|');
    if (sel.dataset.sig !== sig) {
      sel.dataset.sig = sig;
      const groups = [['builtin', 'Pets'], ['user', 'Your pets'], ['codex', 'From Codex'], ['petdex', 'From Petdex']];
      sel.innerHTML = groups
        .map(([src, label]) => {
          const g = state.pets.filter((p) => p.source === src && !p.error);
          return g.length ? `<optgroup label="${esc(label)}">${g.map((p) => `<option value="${esc(p.key)}">${esc(p.name)}</option>`).join('')}</optgroup>` : '';
        })
        .join('');
    }
    if (pet) sel.value = pet.key;
    document.querySelectorAll('#seg-size button').forEach((b) => b.classList.toggle('on', b.dataset.size === state.config.size));
  }

  $('#sel-pet').addEventListener('change', (e) => act('setPet', e.target.value));
  document.querySelectorAll('#seg-size button').forEach((b) => b.addEventListener('click', () => act('setSize', b.dataset.size)));
  $('#btn-toggle').addEventListener('click', () => act('togglePet'));
  $('#btn-corner').addEventListener('click', () => act('moveToCorner'));

  // ----------------------------------------------------------- Claude Code
  function renderHooks() {
    const h = state.hooks;
    $('#hook-dot').className = 'sdot ' + (h === 'full' ? 'on' : h === 'status' ? 'half' : '');
    $('#hook-status').textContent = h === 'full' ? 'Live hooks connected, with answering' : h === 'status' ? 'Live hooks connected (status only)' : 'Live hooks not connected';
    $('#hook-help').textContent = h === 'none'
      ? 'The pet already sees every session from Claude’s status files. Connect hooks for live tool activity, subagents, and Allow / Deny right on the pet. Adds entries to ~/.claude/settings.json (backed up first).'
      : 'Sessions that were already open may need a restart before their live activity shows.';
    $('#btn-connect-full').hidden = h === 'full';
    $('#btn-connect-status').hidden = h === 'status';
    $('#btn-disconnect').hidden = h === 'none';
    $('#chk-answer').checked = !!state.config.answerFromPet;
    $('#sel-hold').value = String(state.config.holdSeconds);
    $('#sel-hold').disabled = !state.config.answerFromPet;
  }
  const HOOK_EVENTS = [
    ['SessionStart / SessionEnd', 'sessions appearing and closing'],
    ['UserPromptSubmit', 'a turn starts, and the title'],
    ['PreToolUse / PostToolUse', 'what each session is doing right now'],
    ['SubagentStart / SubagentStop', 'the subagent list'],
    ['Notification', '“needs your permission” and friends'],
    ['Stop', 'done, with Claude’s last message'],
  ];
  const ANSWER_EVENTS = [
    ['PermissionRequest', 'Allow / Deny from the pet'],
    ['PreToolUse · AskUserQuestion', 'picking answers from the pet'],
  ];

  function renderSettings() {
    const c = state.config;
    const items = [...HOOK_EVENTS, ...(state.hooks === 'full' || state.hooks === 'none' ? ANSWER_EVENTS : [])];
    const list = items.map(([k, v]) => `<li><code>${esc(k)}</code> ${esc(v)}</li>`).join('');
    if ($('#hook-list').innerHTML !== list) $('#hook-list').innerHTML = list;
    $('#chk-sound').checked = !!c.sound;
    $('#chk-quiet').checked = !!c.showQuietSessions;
    $('#pets-path').textContent = state.paths.pets;
    $('#chk-login').checked = !!state.login.enabled;
    $('#chk-login').disabled = !state.login.supported;
    $('#login-note').textContent = state.login.supported ? '' : 'Available in the installed app (not when running from source).';
  }
  // ---- shortcuts + whip
  const SYM = { Command: '⌘', Cmd: '⌘', CommandOrControl: '⌘', CmdOrCtrl: '⌘', Control: '⌃', Ctrl: '⌃', Alt: '⌥', Option: '⌥', Shift: '⇧', Super: '⌘', Meta: '⌘' };
  const pretty = (acc) => (acc || '').split('+').map((k) => SYM[k] || k).join(' ');
  let recording = null;

  function renderShortcuts() {
    if (recording) return;
    for (const name of ['shortcut', 'whipShortcut']) {
      const el = $('#sc-' + name);
      el.textContent = pretty(state.shortcuts[name]);
      el.classList.toggle('bad', state.shortcuts.ok && state.shortcuts.ok[name] === false);
    }
    $('#chk-whip-sound').checked = !!state.config.whipSound;
  }

  const KEYNAME = (e) => {
    const c = e.code;
    if (/^Key[A-Z]$/.test(c)) return c.slice(3);
    if (/^Digit\d$/.test(c)) return c.slice(5);
    if (/^F\d{1,2}$/.test(c)) return c;
    return { Space: 'Space', Slash: '/', Period: '.', Comma: ',', Semicolon: ';', Quote: "'", BracketLeft: '[', BracketRight: ']', Backslash: '\\', Minus: '-', Equal: '=', Backquote: '`', ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right' }[c] || null;
  };

  document.querySelectorAll('[data-rec]').forEach((b) => b.addEventListener('click', () => {
    recording = b.dataset.rec;
    document.querySelectorAll('[data-rec]').forEach((x) => { x.textContent = x === b ? 'Press keys…' : 'Change'; });
    $('#sc-' + recording).textContent = '…';
    $('#sc-msg').textContent = 'Press the combination now (Esc cancels).';
  }));

  window.addEventListener('keydown', async (e) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const name = recording;
    const done = (msg) => {
      recording = null;
      document.querySelectorAll('[data-rec]').forEach((x) => { x.textContent = 'Change'; });
      $('#sc-msg').textContent = msg;
      renderShortcuts();
    };
    if (e.key === 'Escape') return done('Unchanged.');
    const key = KEYNAME(e);
    if (!key) return; // still holding modifiers
    if (!(e.metaKey || e.ctrlKey || e.altKey)) return done('Use ⌘, ⌃ or ⌥ plus a key, so it doesn’t get in the way of typing.');
    const accel = [e.metaKey && 'Command', e.ctrlKey && 'Control', e.altKey && 'Alt', e.shiftKey && 'Shift', key].filter(Boolean).join('+');
    const res = await act('setShortcut', { name, accel });
    done(res && res.ok ? `Saved: ${pretty(accel)}` : (res && res.error) || 'Couldn’t use that one.');
  }, true);

  $('#chk-whip-sound').addEventListener('change', (e) => act('setWhipSound', e.target.checked));

  $('#chk-sound').addEventListener('change', (e) => act('setSound', e.target.checked));
  $('#chk-quiet').addEventListener('change', (e) => act('setShowQuiet', e.target.checked));
  $('#chk-login').addEventListener('change', (e) => act('setLogin', e.target.checked));
  $('#btn-pets-folder').addEventListener('click', () => act('openPetsFolder'));
  $('#btn-pets-install').addEventListener('click', () => act('installPet'));
  $('#btn-pets-reload').addEventListener('click', () => act('reloadPets'));

  $('#btn-connect-full').addEventListener('click', () => act('connectHooks', true));
  $('#btn-connect-status').addEventListener('click', () => act('connectHooks', false));
  $('#btn-disconnect').addEventListener('click', () => act('disconnectHooks'));
  $('#chk-answer').addEventListener('change', (e) => act('setAnswer', e.target.checked));
  $('#sel-hold').addEventListener('change', (e) => act('setHold', Number(e.target.value)));

  // --------------------------------------------------------------- sessions
  const rows = new Map();
  const tbody = $('#sess-table tbody');

  function appLabel(s) {
    const e = s.entrypoint || '';
    if (e === 'claude-desktop') return 'Claude app';
    if (/vscode/i.test(e)) return 'VS Code';
    const t = (s.host && s.host.termProgram) || '';
    if (t === 'Apple_Terminal') return 'Terminal';
    if (t === 'iTerm.app') return 'iTerm';
    return t || (e === 'cli' ? 'Terminal' : '—');
  }

  function doing(s) {
    if (s.state === 'question') return (s.question && s.question.title) || (s.attention && (s.attention.ask ? 'Asking: ' : '') + s.attention.text) || 'Needs you';
    if (s.state === 'working') {
      const bits = [];
      if (s.activity) bits.push(s.activity.text);
      if (s.subagents && s.subagents.length) bits.push(`${s.subagents.length} subagent${s.subagents.length === 1 ? '' : 's'}`);
      return bits.join(' · ') || 'Working';
    }
    if (s.state === 'done') return '✓ ' + ((s.attention && s.attention.text) || 'Ready');
    if (s.state === 'ended') return 'Closed';
    return 'Idle';
  }

  function renderSessions() {
    const list = state.sessions;
    const working = list.filter((s) => s.state === 'working').length;
    $('#sess-count').textContent = `${list.length} total · ${working} working`;
    $('#btn-clear').hidden = !list.some((s) => s.state === 'done' || s.state === 'ended');
    const keep = new Set();
    list.forEach((s, i) => {
      keep.add(s.key);
      let tr = rows.get(s.key);
      if (!tr) {
        tr = document.createElement('tr');
        tr.dataset.key = s.key;
        tr.innerHTML = '<td class="st"><i></i></td><td class="t"></td><td class="d"></td><td class="a"></td><td class="src"></td><td class="u"></td><td class="x"><button title="Close">×</button></td>';
        tr.querySelector('.x button').addEventListener('click', (e) => { e.stopPropagation(); act('dismiss', s.key); });
        tr.addEventListener('click', async () => {
          const res = await act('openSession', s.key);
          if (res && !res.ok) { tr.classList.remove('nope'); void tr.offsetWidth; tr.classList.add('nope'); }
        });
        rows.set(s.key, tr);
      }
      const set = (sel, html) => { const td = tr.querySelector(sel); if (td.innerHTML !== html) td.innerHTML = html; };
      tr.querySelector('.st i').className = s.state;
      set('.t', `<b>${esc(s.title)}</b><small>${esc(s.project || '')}</small>`);
      set('.d', esc(doing(s)));
      set('.a', esc(appLabel(s)));
      set('.src', s.hooked ? (s.pid ? 'status + hooks' : 'hooks') : 'status file');
      set('.u', esc(ago(s.updatedAt)));
      tr.querySelector('.x button').hidden = s.state === 'working' || (s.state === 'question' && !(s.attention && s.attention.ask && !s.question));
      if (tbody.children[i] !== tr) tbody.insertBefore(tr, tbody.children[i] || null);
    });
    for (const [k, tr] of rows) if (!keep.has(k)) { tr.remove(); rows.delete(k); }
    if (!list.length && !tbody.querySelector('.empty-row')) tbody.innerHTML = '<tr class="empty-row"><td colspan="7">No Claude Code sessions running right now.</td></tr>';
    if (list.length) { const e = tbody.querySelector('.empty-row'); if (e) e.remove(); }
  }
  $('#btn-clear').addEventListener('click', () => act('clearFinished'));

  // ------------------------------------------------------------------ debug
  function renderDebug() {
    const d = state;
    const kv = [
      ['Hook server', d.server.ok ? `<span class="ok">● listening</span> on 127.0.0.1:${d.server.port}` : `<span class="bad">● not running</span> ${esc(d.server.error || '')}`],
      ['Whip', d.whip.on ? `on${d.whip.hustleUntil > Date.now() ? ' · <span class="ok">⚡ hustling</span>' : ''}` : 'off'],
      ['Live hooks', d.hooks === 'full' ? '<span class="ok">connected, with answering</span>' : d.hooks === 'status' ? '<span class="ok">connected (status only)</span>' : 'not connected'],
      ['Events', `${d.eventCount} received since launch`],
      ['Sessions', `${d.sessions.length} (${d.sessions.filter((s) => s.state === 'working').length} working)`],
      ['Pet', esc(d.pet ? `${d.pet.name} · ${d.pet.source} · ${d.pet.frameWidth}×${d.pet.frameHeight}` : '—')],
      ['Data folder', esc(d.paths.data)],
      ['Pets folder', esc(d.paths.pets)],
      ['Hook script', esc(d.paths.hookScript)],
      ['Claude settings', esc(d.paths.claudeSettings)],
      ['Claude sessions', esc(d.paths.claudeSessions)],
      ['App', esc(`Promptagotchi ${d.version} · ${d.packaged ? 'installed app' : 'from source'}`)],
      ['Runtime', esc(d.runtime || '')],
      ['System', esc(d.os || '')],
    ];
    const html = kv.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join('');
    if ($('#debug-kv').innerHTML !== html) $('#debug-kv').innerHTML = html;
    $('#btn-sim').textContent = d.simulator.running ? 'Stop simulator' : 'Start simulator';
    $('#btn-sim').className = d.simulator.running ? 'danger' : '';
    $('#sel-sim').disabled = d.simulator.running;
  }
  $('#btn-sim').addEventListener('click', () => act(state.simulator.running ? 'stopSim' : 'startSim', Number($('#sel-sim').value)));
  $('#btn-data').addEventListener('click', () => act('openData'));
  $('#btn-settings').addEventListener('click', () => act('openSettings'));
  $('#btn-diag').addEventListener('click', async () => {
    await act('copyDiagnostics');
    const b = $('#btn-diag');
    b.textContent = 'Copied ✓';
    setTimeout(() => { b.textContent = 'Copy diagnostics'; }, 1500);
  });

  // ------------------------------------------------------------ live events
  const log = $('#event-log');
  let firstEvent = true;
  const CATS = {
    PreToolUse: 'tools', PostToolUse: 'tools', PostToolUseFailure: 'tools',
    UserPromptSubmit: 'prompts',
    Stop: 'stops',
    SubagentStart: 'agents', SubagentStop: 'agents',
    Notification: 'attention', PermissionRequest: 'attention',
    SessionStart: 'sessions', SessionEnd: 'sessions',
  };
  document.querySelectorAll('#ev-filters button').forEach((b) => b.addEventListener('click', () => {
    b.classList.toggle('on');
    log.classList.toggle('hide-' + b.dataset.cat, !b.classList.contains('on'));
    if ($('#chk-follow').checked) log.scrollTop = log.scrollHeight;
  }));
  $('#btn-ev-clear').addEventListener('click', () => { log.innerHTML = ''; });

  function addEvents(evs) {
    if (firstEvent && evs.length) { log.innerHTML = ''; firstEvent = false; }
    for (const e of evs) {
      const row = document.createElement('div');
      row.className = 'ev' + (e.kind ? ' ' + e.kind : '');
      row.dataset.cat = CATS[e.name] || 'sessions';
      const t = new Date(e.at);
      row.innerHTML = `<time>${t.toLocaleTimeString([], { hour12: false })}</time><b>${esc(e.name)}</b><span class="who">${esc(e.who)}</span><span class="what">${esc(e.what || '')}</span>`;
      log.append(row);
    }
    while (log.children.length > 400) log.firstChild.remove();
    if ($('#chk-follow').checked) log.scrollTop = log.scrollHeight;
  }
  window.control.on('control:events', addEvents);

  // ------------------------------------------------------------------ wiring
  async function act(name, arg) {
    return window.control.act(name, arg);
  }

  function renderAll() {
    if (!state) return;
    renderPet();
    renderHooks();
    renderSettings();
    renderShortcuts();
    renderSessions();
    renderDebug();
  }

  window.control.on('control:state', (s) => { state = s; renderAll(); });
  window.control.get().then((s) => {
    state = s;
    renderAll();
    addEvents(s.recentEvents || []);
  });
  setInterval(() => { if (state) renderSessions(); }, 5000); // keep "updated" times fresh
})();
