// Command rows keep their compact form and open to the command and output.
(() => {
  const opened = new Map();
  const commandRow = call => {
    const open = opened.get(call.id) ?? call.open ?? false;
    const status = call.status || 'Completed';
    const running = status === 'Running';
    const failed = status === 'Failed';
    const body = open ? `<div class="call-details"><dl><dt>Tool</dt><dd>Bash</dd></dl><h4>Command</h4><pre>${esc(call.command || 'Command unavailable.')}</pre><h4>Output</h4>${call.outputHtml || `<pre>${esc(call.output || (running ? 'No output yet.' : 'No output recorded.'))}</pre>`}</div>` : '';
    return `<div class="dcoderen tool row${open ? ' open' : ''}${failed ? ' bad' : ''}" data-tool-call="${esc(call.id)}" tabindex="0" role="button" aria-expanded="${open}"><span class="ki">${AG_IC.run}</span><span class="vb">${running ? 'Running' : 'Ran'}</span><code class="tt" title="${esc(call.description)}">${esc(call.description)}</code><span class="rm">${esc(call.detail || '')}</span><span class="mk">${running ? '<span class="dspin"></span>' : failed ? AG_IC.bad : AG_IC.ok}</span>${body}</div>`;
  };
  const testCommand = () => commandRow({
    id: 'session-tests', description: 'cargo test -p widget-core', command: 'cargo test -p widget-core',
    output: 'test result: ok. 24 passed; 0 failed.',
  });
  const buildCommand = () => commandRow({
    id: 'session-build', description: 'cargo check --target aarch64-apple-ios', command: 'cargo check --target aarch64-apple-ios',
    status: 'Running', detail: '17s',
  });
  const waitCommand = (id = 'session-wait', status = 'Running') => commandRow({
    id, description: 'Timer to recheck the iOS build', command: 'sleep 45', status,
    detail: status === 'Running' ? '8s' : '',
    output: status === 'Completed' ? 'Finished waiting.' : status === 'Failed' ? 'Interrupted by user.' : '',
  });
  const conversation = () => dYou('Fix the failing login test and check the iOS build')
    + '<div class="dcoderen agent md">I found the old error message in the test. I will update it and check both builds.</div>'
    + dRows(dTool('Read src/login.rs', 'ok', 'read')
      + dTool('Searched Wrong password in tests/', 'ok', 'search')
      + dTool('Edited tests/login.rs', 'ok', 'edit')
      + testCommand() + buildCommand())
    + '<div class="dcoderen agent md">All 24 login tests pass. The iOS check is still running. I will check it again shortly.</div>'
    + '<div class="dcoderen compacted"><span>Conversation compacted</span></div>'
    + dRows(waitCommand());

  const session = D.dAgentSession;
  if (!S.d.ags) S.d.ags = 'toolcalls';
  D.dAgentSession = () => {
    const mode = S.d.ags;
    const sample = mode === 'toolcalls';
    if (sample) S.d.ags = 'working';
    const template = document.createElement('template');
    try { template.innerHTML = session(); } finally { S.d.ags = mode; }
    const log = template.content.querySelector('.dcoderlog');
    if (!log) return template.innerHTML;
    if (sample) {
      log.innerHTML = conversation();
      template.content.querySelectorAll('.dcard.dcoder, .agtasks').forEach(el => el.remove());
    } else {
      // Read, search and edit rows keep their existing display.
      log.querySelectorAll('.dcoderen.tool.row').forEach((row, index) => {
        const verb = row.querySelector('.vb')?.textContent || '';
        if (!/^(Run|Ran|Running)$/.test(verb)) return;
        const target = row.querySelector('.tt');
        const command = target?.getAttribute('title') || target?.textContent || '';
        const replacement = document.createElement('template');
        replacement.innerHTML = commandRow({
          id: `session-${mode}-${index}`, command, description: command,
          status: row.classList.contains('bad') ? 'Failed' : row.querySelector('.dspin') ? 'Running' : 'Completed',
          detail: row.querySelector('.rm')?.textContent || '', open: row.classList.contains('open'),
          outputHtml: row.querySelector('.bdy')?.innerHTML || '',
        });
        row.replaceWith(replacement.content);
      });
    }
    return template.innerHTML;
  };
  VARIANTS.dAgentSession = VARIANTS.dAgentSession.map(([label, activate, active]) => [label, activate, () => S.d.ags !== 'toolcalls' && active()]);
  VARIANTS.dAgentSession.unshift(['Command details', () => { S.d.ags = 'toolcalls'; render(); }, () => S.d.ags === 'toolcalls']);

  D.dAgentToolDetails = () => {
    const template = document.createElement('template');
    template.innerHTML = D.dAgentActivity();
    const state = S.d.callState || 'running';
    const status = { running: 'Running', completed: 'Completed', cancelled: 'Failed', unknown: 'Running' }[state];
    const call = state === 'unknown'
      ? commandRow({ id: 'preview-wait', description: 'Recheck the build', status })
      : waitCommand('preview-wait', status);
    template.content.querySelector('.dcoderlog').innerHTML = dYou('Check the build when it finishes')
      + '<div class="dcoderen agent md">The build is still running. I will check again shortly.</div>' + call;
    if (S.d.callTray === '1') {
      const log = template.content.querySelector('.dcoderlog').innerHTML;
      return deskScene(dBar('<div class="dinput small"><span class="txt">Build check</span><span class="dmuted">Claude Code</span></div>'
        + `<div class="dbody"><div class="dcoderlog">${log}</div></div>`
        + '<div class="dinput small"><input placeholder="Message Claude Code" aria-label="Message Claude Code"></div>' + dHints()));
    }
    return template.innerHTML;
  };
  VARIANTS.dAgentToolDetails = ['running', 'completed', 'cancelled', 'unknown'].map(value => [value[0].toUpperCase() + value.slice(1), () => { S.d.callState = value; render(); }, () => (S.d.callState || 'running') === value]);
  VARIANTS.dAgentToolDetails.push(['Tray', () => { S.d.callTray = S.d.callTray === '1' ? '0' : '1'; render(); }, () => S.d.callTray === '1']);
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dAgentToolDetails', 'Agent: command details', 'AgentToolDetails']);

  document.addEventListener('click', event => {
    const row = event.target.closest('[data-tool-call]');
    if (!row || event.target.closest('.call-details')) return;
    event.preventDefault();
    event.stopPropagation();
    const id = row.dataset.toolCall;
    opened.set(id, row.getAttribute('aria-expanded') !== 'true');
    render();
    document.querySelector(`[data-tool-call="${CSS.escape(id)}"]`)?.focus({ preventScroll: true });
  }, true);
  document.addEventListener('keydown', event => {
    if (!event.target.matches('[data-tool-call]') || !['Enter', ' '].includes(event.key)) return;
    event.preventDefault();
    event.target.click();
  });
  if (['dAgentSession', 'dAgentToolDetails'].includes(location.hash.slice(1))) {
    S.screen = location.hash.slice(1);
    render();
  }
})();
