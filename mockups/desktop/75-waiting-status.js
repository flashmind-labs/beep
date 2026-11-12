// ---- Agent session: waiting on background work (#603) ----
// The agent replied, but a background subagent or command still runs, so the
// turn has not ended. Where "Working" sat, one still line says what it waits
// for, naming the oldest: "Waiting for 1 subagent · Upgrade all deps · 12 min".
// The time shows from a minute on and moves once a minute. A click opens that
// subagent, or the Running tab for a command. The line goes once nothing runs.
(() => {
  const CASES = {
    waitsub: { label: 'Waiting for 1 subagent', name: 'Upgrade all deps', time: '12 min', open: 'subchild', tip: 'Open Upgrade all deps' },
    waitcmd: { label: 'Waiting for 1 command', name: 'Run tests', time: '', open: 'running', tip: 'Show it in the Running tab' },
    waitmany: { label: 'Waiting for 2 subagents and 1 command', name: 'Upgrade all deps', time: '12 min', open: 'subchild', tip: 'Open Upgrade all deps' },
  };
  const line = c => `<div class="dthink dlive"><button data-dact="dset:ags:${c.open}" title="${c.tip}"><span class="lb">${c.label}</span><span class="ln">· ${c.name}</span>${c.time ? `<span class="since">· ${c.time}</span>` : ''}</button></div>`;
  const conversation = c => dYou('Upgrade the dependencies and run the tests')
    + dRows(dTool('Read Cargo.toml', 'ok', 'read') + dTool('Started subagent Upgrade all deps', 'ok', 'search') + dTool('Ran cargo test in the background', 'ok', 'run'))
    + '<div class="dcoderen agent md">The upgrade runs in a subagent and the tests in the background. I will report when both finish.</div>'
    + line(c);

  const session = D.dAgentSession;
  D.dAgentSession = () => {
    const c = CASES[S.d.ags];
    if (!c) return session();
    const mode = S.d.ags;
    S.d.ags = 'working';
    const template = document.createElement('template');
    try { template.innerHTML = session(); } finally { S.d.ags = mode; }
    const log = template.content.querySelector('.dcoderlog');
    if (log) log.innerHTML = conversation(c);
    template.content.querySelectorAll('.dcard.dcoder').forEach(el => el.remove());
    return template.innerHTML;
  };
  VARIANTS.dAgentSession.push(...[['Waiting for a subagent', 'waitsub'], ['Waiting for a command', 'waitcmd'], ['Waiting for several', 'waitmany']]
    .map(([label, value]) => [label, () => { S.d.ags = value; render(); }, () => S.d.ags === value]));
})();
