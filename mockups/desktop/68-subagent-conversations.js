// Native subagents use the session shell and transcript components.
(() => {
  const sessions = [
    { id: 'auth', title: 'Explore auth module', prompt: 'Find where the login error text is set. Explain which tests assert on it.', state: 'idle', time: '41s', tokens: '8.2k', cost: '$0.03', agent: 'claude', reads: ['src/login.rs', 'src/errors.rs', 'tests/login.rs'], reply: '<p>The error text comes from <code>LoginError::Display</code> in <code>src/errors.rs</code>.</p><p><code>tests/login.rs</code> asserts on it twice. The fixture still expects <code>Wrong password</code>, while the implementation returns <code>Wrong email or password</code>.</p>' },
    { id: 'ios', title: 'Check the iOS copy', prompt: 'Look for the same string in the iOS app. Check the label and the UI test before suggesting a change.', state: 'working', time: '18s', tokens: '3.6k', cost: '$0.02', agent: 'claude', reads: ['ios/LoginView.swift', 'ios/Localizable.strings'], reply: '<p>The login screen uses the shared error string. I am checking whether the UI test overrides it.</p>' },
    { id: 'fixtures', title: 'Search old fixtures', prompt: 'List fixtures that assert on the login error text. Include their paths and expected strings.', state: 'working', time: '12s', tokens: '2.1k', cost: '$0.01', agent: 'claude', reads: ['tests/fixtures/login.json', 'tests/login.rs'], reply: '<p>Two fixtures still use the old message. I am checking the snapshot assertions too.</p>' },
    { id: 'release', title: 'Check the release notes', prompt: 'Compare the changelog with the diff. Check whether the login copy change needs a release note.', state: 'idle', time: '32s', tokens: '5.7k', cost: '$0.02', agent: 'claude', reads: ['CHANGELOG.md', 'src/errors.rs'], reply: '<p>The changelog already mentions the updated login message. No additional release note is needed.</p>' },
    { id: 'codex', title: 'Review login assertions', prompt: 'Review the login assertions against the current error text. Report the exact mismatch without changing files.', state: 'working', time: '24s', tokens: '4.3k', cost: '$0.02', agent: 'codex', reads: ['src/errors.rs', 'tests/login.rs'], reply: '<p>The implementation and the fixture disagree. I am checking the remaining assertions before reporting back.</p>' },
  ];
  const previews = { subchild: 'auth', subworking: 'ios', subcodex: 'codex' };
  let current = null, parent = 'subagents', parentScroll = 0;
  const escape = text => text.replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const get = id => sessions.find(session => session.id === id);
  const isWorking = session => session.state === 'working';
  const open = (session, origin = S.d.ags) => {
    parent = origin === 'disksubs' ? origin : 'subagents';
    parentScroll = document.getElementById('agbody')?.scrollTop || 0;
    current = session.id;
    S.d.ags = isWorking(session) ? 'working' : 'idle';
    render();
    document.getElementById('agbody').scrollTop = 0;
    document.querySelector('[data-sub-parent]')?.focus({ preventScroll: true });
  };
  const back = () => {
    const title = get(current)?.title;
    current = null;
    S.d.ags = parent;
    render();
    document.getElementById('agbody').scrollTop = parentScroll;
    [...document.querySelectorAll('[data-open-sub]')].find(row => row.dataset.openSub === title)?.focus({ preventScroll: true });
  };
  const tool = (session, index, path) => {
    const key = `sub-${session.id}-tool-${index}`, expanded = !!S.d[key];
    return `<div class="dcoderen tool row${expanded ? ' open' : ''}" role="button" tabindex="0" data-sub-toggle="${key}" aria-expanded="${expanded}"><span class="ki">${AG_IC.read}</span><span class="vb">Read</span><code class="tt">${path}</code><span class="rm"></span><span class="mk">${AG_IC.ok}</span>${expanded ? `<div class="bdy"><pre class="box">${escape(path.endsWith('errors.rs') ? 'impl Display for LoginError {\n    // Shared login error message.\n    // Wrong email or password\n}' : 'Checked the login error text and its assertions.')}</pre></div>` : ''}</div>`;
  };
  const transcript = session => {
    const working = isWorking(session), key = `sub-${session.id}-work`, expanded = !!S.d[key];
    const from = `<div class="dcoderen fromsession"><b>Fix the failing login test</b><span>${session.prompt}</span></div>`;
    const thoughtKey = `sub-${session.id}-thought`;
    const thought = dThink({ secs: 6, open: !!S.d[thoughtKey], act: `dset:${thoughtKey}:${S.d[thoughtKey] ? '' : '1'}`, text: 'I will compare the error message with the assertions, then report the mismatch to the parent.' });
    const work = dHead(`Read ${session.reads.length} files`, { act: `dset:${key}:${expanded ? '' : '1'}`, open: expanded });
    return from + dHead(`${working ? 'Working' : 'Worked'} for ${session.time}`, { when: '17:14' }) + thought + work
      + (expanded ? dRows(session.reads.map((path, index) => tool(session, index, path)).join('')) : '')
      + `<div class="dcoderen agent md">${session.reply}</div>`
      + (working ? dLive('Working', session.time, { doing: 'Checking the remaining assertions' }) : '');
  };
  const base = D.dAgentSession;
  D.dAgentSession = () => {
    if (S.screen !== 'dAgentSession') return base();
    if (previews[S.d.ags]) {
      current = previews[S.d.ags];
      parent = 'subagents';
    }
    const session = get(current);
    if (!session) return base();
    const previous = S.d.ags;
    const normal = previews[previous] ? (isWorking(session) ? 'working' : 'idle') : previous;
    S.d.ags = normal;
    const template = document.createElement('template');
    try { template.innerHTML = base(); } finally { S.d.ags = previous; }
    const root = template.content;
    const title = root.querySelector('.agtitle');
    title.dataset.sessionKey = session.id;
    title.dataset.originalTitle = session.title;
    title.textContent = session.title;
    root.querySelector('.dagmain').innerHTML = `<nav class="sub-parent" aria-label="Conversation ancestry"><button type="button" data-sub-parent title="Open parent conversation (Esc)">Fix the failing login test</button><span aria-hidden="true">/</span><span aria-current="page">${session.title}</span><button type="button" class="dbtn quiet small" data-sub-parent>Open parent<kbd>esc</kbd></button></nav><div class="dcoderlog">${transcript(session)}</div>`;
    root.querySelector('#aguse').textContent = `8%/200k · ${session.tokens} tokens · ${session.cost}`;
    root.querySelector('#agstate').innerHTML = `<span class="dstate${isWorking(session) ? ' run' : ''}">${isWorking(session) ? 'Working' : 'Idle'}</span>`;
    root.querySelector('.agtasks')?.remove();
    const name = session.agent === 'codex' ? 'Codex' : 'Claude Code';
    const mark = `<span class="cc-ag ${session.agent} mark">${window.__MARKS[session.agent]}</span>`;
    root.querySelector('.dhead > .dcoderav, .dhead > .cc-ag')?.remove();
    root.querySelector('.dhead').insertAdjacentHTML('afterbegin', mark);
    const model = root.querySelector('#agmodelb');
    model.innerHTML = `${mark}${name} <small>Default</small>`;
    model.removeAttribute('data-dact');
    model.disabled = true;
    model.title = 'Model set by the parent when this subagent started';
    const field = root.querySelector('#agfield');
    field.querySelector('.txt').outerHTML = `<textarea class="sub-message" aria-label="Message ${session.title}" placeholder="Open the parent conversation to send a message" rows="1" disabled></textarea>`;
    field.querySelector('#agstop')?.remove();
    field.insertAdjacentHTML('beforeend', '<button class="dbtn" data-sub-parent>Open parent</button>');
    root.querySelector('.dhints.slim').innerHTML = '<span><kbd>esc</kbd>parent session</span>';
    return template.innerHTML;
  };
  // Capture these before the shared navigation handlers.
  document.addEventListener('click', event => {
    if (S.screen !== 'dAgentSession') return;
    const row = event.target.closest('[data-open-sub]');
    if (row) {
      const session = sessions.find(item => item.title === row.dataset.openSub);
      if (session) { event.stopImmediatePropagation(); open(session); }
      return;
    }
    if (!current) return;
    if (event.target.closest('[data-sub-parent]')) { event.stopImmediatePropagation(); back(); }
    const toggle = event.target.closest('[data-sub-toggle]');
    if (toggle) { S.d[toggle.dataset.subToggle] = !S.d[toggle.dataset.subToggle]; render(); }
    if (event.target.closest('[data-screen], [data-dgo]')) current = null;
  }, true);
  document.addEventListener('keydown', event => {
    if (S.screen !== 'dAgentSession' || !current || event.isComposing) return;
    if (event.key === 'Escape' && !event.target.matches('.agtitle-input')) {
      event.preventDefault(); event.stopImmediatePropagation(); back();
    } else if (event.target.matches('[data-sub-toggle]') && ['Enter', ' '].includes(event.key)) {
      event.preventDefault(); event.target.click();
    }
  }, true);
  VARIANTS.dAgentSession = VARIANTS.dAgentSession.map(([label, choose, active]) => [label, () => { current = null; choose(); }, () => current ? label === 'Subagent opened' && current === 'auth' : active()]);
  const at = VARIANTS.dAgentSession.findIndex(([label]) => label === 'Subagent opened');
  VARIANTS.dAgentSession[at][0] = 'Subagent conversation, completed';
  VARIANTS.dAgentSession.splice(at + 1, 0, ...[['Subagent conversation, working', 'subworking'], ['Codex subagent conversation', 'subcodex']].map(([label, variant]) => [label, () => { current = null; S.d.ags = variant; render(); }, () => current === previews[variant]]));
  if (location.hash === '#subagent-conversation') {
    S.screen = 'dAgentSession';
    S.d.ags = 'subchild';
  }
  render();
})();
