// ---- Agent session: the Device tab (#648) ----
// An Android emulator or iOS simulator on any machine of the mesh, live in
// the right dock. One picker lists every machine's devices; it starts on the
// session's machine when that has any. The chip says off, booting or on, with
// Boot or Shut down beside it. The screen keeps its aspect and fits the dock;
// a click taps, a drag swipes or scrolls, and with the screen focused the
// keyboard types into the device. The button row presses its hardware buttons.
// The stream stops while the tab or the window is hidden.
(() => {
  const DEVICES = [
    ['build-box', [['Pixel 9 Pro', 'Android 16'], ['Pixel Tablet', 'Android 16']]],
    ['mac-box', [['iPhone 17', 'iOS 26.5'], ['Pixel 8', 'Android 15']]],
  ];
  const CASES = {
    devon: { state: 'on', chip: 'run', label: 'on', act: 'Shut down' },
    devoff: { state: 'off', chip: '', label: 'off', act: 'Boot' },
    devboot: { state: 'booting', chip: 'wait', label: 'booting', act: 'Shut down' },
    devended: { state: 'ended', chip: 'bad', label: 'on', act: 'Shut down' },
    devnone: { state: 'none' },
  };
  const ICON = {
    home: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="7"/></svg>',
    back: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M16 5L7 12l9 7z"/></svg>',
    recent: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>',
    rotate: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5"/></svg>',
    power: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 3v8M6.3 7.3a8 8 0 1 0 11.4 0"/></svg>',
    voldown: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 10h4l5-4v12l-5-4H4z"/><path d="M17 12h4" stroke-linecap="round"/></svg>',
    volup: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 10h4l5-4v12l-5-4H4z"/><path d="M17 12h4M19 10v4" stroke-linecap="round"/></svg>',
    tab: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="7" y="3" width="10" height="18" rx="2"/><path d="M11 18h2" stroke-linecap="round"/></svg>',
  };
  const BUTTONS = [['home', 'Home'], ['back', 'Back'], ['recent', 'Recent apps'], null, ['rotate', 'Rotate'], null, ['power', 'Power'], ['voldown', 'Volume down'], ['volup', 'Volume up']];

  const picker = () => `<select class="ag-devpick" aria-label="Device">${DEVICES.map(([machine, list]) => `<optgroup label="${machine}">${list.map(([name], i) => `<option${machine === 'build-box' && i === 0 ? ' selected' : ''}>${name}</option>`).join('')}</optgroup>`).join('')}</select>`;
  // A stand-in for the frame the node sends: the real view draws JPEGs on a canvas.
  const screen = () => `<div class="ag-devscreen" tabindex="0" title="Click to tap, drag to swipe; type while it is focused"><div class="sb"><span>9:41</span><span>5G</span></div><div class="clock">9:41<small>Thursday, 12 November</small></div><div class="apps">${'<i></i>'.repeat(8)}</div><div class="tray">${'<i></i>'.repeat(4)}</div></div>`;
  const stage = c => {
    if (c.state === 'on') return screen();
    if (c.state === 'booting') return '<div class="ag-devnote"><span class="dspin"></span>Booting Pixel 9 Pro…</div>';
    if (c.state === 'ended') return '<div class="ag-devnote">The view of Pixel 9 Pro ended: the emulator closed.<button class="dbtn" data-dact="dset:ags:devon">Show again</button></div>';
    return '<div class="ag-devnote">Pixel 9 Pro is off.</div>';
  };
  const body = c => {
    if (c.state === 'none') return '<div class="ag-pbody"><div class="ag-devnote">No emulator or simulator on build-box or mac-box.</div></div>';
    const head = `<div class="ag-devhead">${picker()}<span class="chip ${c.chip}">${c.state === 'booting' ? '<span class="dspin"></span>' : ''}${c.label}</span><span class="sp"></span><button class="dbtn" data-dact="dset:ags:${c.act === 'Boot' ? 'devboot' : 'devoff'}">${c.act}</button></div>`;
    const live = c.state === 'on';
    const row = BUTTONS.map(b => b ? `<button class="dbtn ibtn" title="${b[1]}" aria-label="${b[1]}"${live ? ` data-toast="Pressed ${b[1]}"` : ' disabled'}>${ICON[b[0]]}</button>` : '<span class="gap"></span>').join('');
    return `${head}<div class="ag-pbody stage ag-devstage">${stage(c)}</div><div class="ag-pacts ag-devkeys">${row}<span class="sp"></span><span class="meta">Android 16 · build-box</span></div>`;
  };

  const session = D.dAgentSession;
  D.dAgentSession = () => {
    const c = CASES[S.d.ags];
    if (!c) return session();
    const mode = S.d.ags;
    S.d.ags = 'running';
    const template = document.createElement('template');
    try { template.innerHTML = session(); } finally { S.d.ags = mode; }
    const pane = template.content.querySelector('.ag-pane-content');
    if (!pane) return template.innerHTML;
    const head = pane.querySelector('.ag-phead');
    head.querySelectorAll('.ag-tab.on').forEach(tab => tab.classList.remove('on'));
    head.querySelector('.ag-tabs').insertAdjacentHTML('beforeend', `<button class="ag-tab on" title="Device" data-dact="dset:ags:devon">${ICON.tab}<span>Device</span></button>`);
    pane.replaceChildren(head);
    pane.insertAdjacentHTML('beforeend', body(c));
    return template.innerHTML;
  };
  VARIANTS.dAgentSession.push(...[['Device, on', 'devon'], ['Device, off', 'devoff'], ['Device, booting', 'devboot'], ['Device, view ended', 'devended'], ['Device, none on the machines', 'devnone']]
    .map(([label, value]) => [label, () => { S.d.ags = value; render(); }, () => S.d.ags === value]));
})();
