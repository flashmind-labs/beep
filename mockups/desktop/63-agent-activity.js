// Keep the session UI, grouping thoughts and calls between agent outputs.
(() => {
  const row = (kind, verb, target, running = false) => `<div class="dcoderen tool row"><span class="ki">${AG_IC[kind]}</span><span class="vb">${verb}</span><code class="tt">${target}</code><span class="rm">${running ? '17s' : ''}</span><span class="mk">${running ? '<span class="dspin"></span>' : AG_IC.ok}</span></div>`;
  const progress = (id, summary, inner, preview = '') => {
    const open = S.d[id] ? S.d[id] === '1' : S.d.activity === 'expanded';
    const action = `dset:${id}:${open ? '0' : '1'}`;
    const collapse = open ? `<button class="dprogress-collapse" data-dact="${action}" aria-label="Collapse activity" title="Collapse activity" aria-expanded="true">${AG_IC.chev}</button>` : '';
    return `<section class="activity-progress">${dHead(summary, { act: action, open })}${open ? dRows(inner) + collapse : preview ? dRows(preview) : ''}</section>`;
  };
  D.dAgentActivity = () => {
    const previous = S.d.ags;
    S.d.ags = 'thoughts';
    const template = document.createElement('template');
    template.innerHTML = D.dAgentSession();
    S.d.ags = previous;
    const first = dThink({ secs: 8, open: true, text: 'I’ll check the failing assertion and the error text.' })
      + row('run', 'Ran', 'cargo test -p widget-core login')
      + dThink({ secs: 6, open: true, text: 'The assertion uses the old copy. I’ll check where the message changed.' })
      + row('read', 'Read', 'src/errors.rs')
      + row('search', 'Searched', 'Wrong password in tests/');
    const working = S.d.activity === 'working';
    const latest = row('run', working ? 'Running' : 'Ran', 'cargo test -p widget-core', working);
    const second = dThink({ secs: 13, open: true, text: 'The copy changed on purpose. The fixture needs to match it.' })
      + row('edit', 'Edited', 'tests/login.rs')
      + dThink({ secs: 6, open: true, text: 'I’ll rerun the suite to check the updated fixture.' })
      + latest;
    template.content.querySelector('.dcoderlog').innerHTML = dYou('Fix the failing login test')
      + progress('activityFirst', 'Thought for 14s · Ran 1 command, read 1 file, searched once', first)
      + '<div class="dcoderen agent md">The test expects the old error message. I’ll update the fixture and run the suite again.</div>'
      + progress('activitySecond', 'Thought for 19s · Edited 1 file, ran 1 command', second, working ? latest : '')
      + (working ? dLive('Working', '48s') : '<div class="dcoderen agent md">Updated the fixture to match <code>Wrong email or password</code>. All 24 tests pass.</div>');
    template.content.querySelector('#agbody').classList.add('activity-preview');
    template.content.querySelectorAll('.dthink > button:not([data-dact])').forEach(button => {
      button.setAttribute('data-activity-thought', '');
      button.setAttribute('aria-expanded', 'true');
    });
    return template.innerHTML;
  };
  document.addEventListener('click', event => {
    const button = event.target.closest('[data-activity-thought]');
    if (!button) return;
    const open = button.parentElement.classList.toggle('open');
    button.setAttribute('aria-expanded', String(open));
  });
  VARIANTS.dAgentActivity = [['Collapsed', 'collapsed'], ['Expanded', 'expanded'], ['Working', 'working']].map(([label, value]) => [label, () => { S.d.activity = value; S.d.activityFirst = ''; S.d.activitySecond = ''; render(); }, () => (S.d.activity || 'collapsed') === value]);
  VARIANTS.dAgentActivity.push(['Finish turn', () => { S.d.activity = 'collapsed'; render(); }, () => false]);
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dAgentActivity', 'Agent: accumulated activity', 'AgentActivity']);
  if (location.hash.slice(1) === 'dAgentActivity') { S.screen = 'dAgentActivity'; render(); }
})();
