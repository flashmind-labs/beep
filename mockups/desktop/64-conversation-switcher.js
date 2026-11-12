// Option+Tab chooses an active conversation. Nothing opens until Option is released.
// A session's subagents sit indented under it, in the order they started.
(() => {
  const conversations = [
    { title: 'Fix the failing login test', project: 'widget', agent: 'Claude Code', mark: 'CC', status: 'Working', mode: 'working', prompt: 'Fix the failing login test', reply: 'Checking the login assertion and running the test suite.', steers: ['Use the existing fixture'] },
    { title: 'Build the iOS target', project: 'widget', agent: 'Codex', mark: 'CX', status: 'Needs you', mode: 'asks', prompt: 'Build the iOS target', reply: 'The simulator build passed. Should I also build for a connected device?' },
    { title: 'Retry interrupted uploads', project: 'sdk', agent: 'Claude Code', mark: 'CC', status: 'Working', mode: 'working', prompt: 'Retry interrupted uploads', reply: 'Adding a retry for interrupted uploads, then checking cancellation.' },
    { title: 'Review the settings layout', project: 'widget', agent: 'Codex', mark: 'CX', status: 'Ready', mode: 'idle', prompt: 'Review the settings layout', reply: 'The layout review is ready. The shortcut labels fit at the smallest window size.' },
    { title: 'Find the login fixtures', project: 'widget', agent: 'Claude Code', mark: 'CC', status: 'Done', mode: 'idle', parent: 0, prompt: 'Find the login fixtures', reply: 'The fixtures are in tests/fixtures/login.json.' },
    { title: 'Run the login tests', project: 'widget', agent: 'Claude Code', mark: 'CC', status: 'Working', mode: 'working', parent: 0, prompt: 'Run the login tests', reply: 'Running the login tests.' },
  ];
  // Top-level ids by use, each followed by its subagents.
  const subs = id => conversations.flatMap((c, i) => c.parent === id ? [i] : []);
  const rows = () => order.flatMap(id => [id, ...subs(id)]);
  const top = id => conversations[id].parent ?? id;
  let current = 0, selected = 1, open = true, order = [0, 1, 2, 3];
  let previousFocus = null;
  const panel = () => `<div class="conversation-switcher-layer" data-switcher-backdrop><section class="conversation-switcher" role="dialog" aria-modal="true" aria-label="Switch active conversation" tabindex="-1">
    <header><b>Active conversations</b><small>Most recently used</small></header>
    <div class="conversation-switcher-items" role="listbox" aria-label="Active conversations" aria-activedescendant="conversation-choice-${selected}">
      ${rows().map(id => { const c = conversations[id]; return `<button id="conversation-choice-${id}" class="conversation-choice${c.parent === undefined ? '' : ' sub'}" role="option" aria-selected="${id === selected}" data-conversation-choice="${id}" tabindex="-1">
        <span class="conversation-mark" aria-hidden="true"><span class="cc-ag ${c.agent === 'Codex' ? 'codex' : 'claude'} mark">${window.__MARKS[c.agent === 'Codex' ? 'codex' : 'claude']}</span><span class="conversation-status ${c.mode}"></span></span>
        <span class="conversation-label"><b>${c.title}</b>${c.parent === undefined ? `<small>${c.project} · ${c.agent}</small>` : ''}</span><span class="conversation-state">${c.status}${id === current ? '<small>Current</small>' : ''}</span>
      </button>`; }).join('')}
    </div>
    <footer><span>Hold Option, press Tab to cycle</span><span>Release to open · Shift to reverse · Esc to cancel</span></footer>
  </section></div>`;
  function draw() {
    document.querySelector('.conversation-switcher-layer')?.remove();
    if (open) {
      document.getElementById('desk').insertAdjacentHTML('beforeend', panel());
      document.querySelector('.conversation-switcher').focus({ preventScroll: true });
      document.getElementById(`conversation-choice-${selected}`).scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }
    renderVariants();
  }
  function begin() {
    previousFocus = document.activeElement;
    open = true;
    selected = current;
  }
  function step(direction) {
    const list = rows();
    selected = list[(list.indexOf(selected) + direction + list.length) % list.length];
    draw();
  }
  function close(commit) {
    open = false;
    if (commit) {
      current = selected;
      order = [top(current), ...order.filter(id => id !== top(current))];
      go('dConversationSwitcher');
    } else {
      draw();
      previousFocus?.focus({ preventScroll: true });
    }
  }
  D.dConversationSwitcher = () => {
    const c = conversations[current], saved = S.d.ags;
    S.d.ags = c.mode;
    const template = document.createElement('template');
    template.innerHTML = D.dAgentSession();
    S.d.ags = saved;
    template.content.querySelector('.dagmain').innerHTML = `<div class="dcoderlog">${dYou(c.prompt)}<div class="dcoderen agent md">${c.reply}</div></div>`;
    template.content.querySelectorAll('.ag-queue').forEach(el => el.remove());
    const queue = (c.steers || []).map(text => `<div class="ag-queue steer"><span class="lbl">Steer</span><span class="t">${esc(text)}</span><span class="w">after the current step</span><button data-toast="Edit message">Edit</button><button data-toast="Not sent">&#215;</button></div>`).join('');
    template.content.querySelector('#agfield').insertAdjacentHTML('beforebegin', queue);
    template.content.querySelectorAll('.ag-pane, .ag-div, .ag-pstrip, .agtasks').forEach(el => el.remove());
    template.content.querySelector('.agproj').textContent = c.project;
    template.content.querySelector('#agstate').textContent = c.status;
    const avatar = template.content.querySelector('.dhead .dcoderav');
    if (avatar) avatar.textContent = c.mark;
    const text = document.createTreeWalker(template.content, NodeFilter.SHOW_TEXT);
    while (text.nextNode()) text.currentNode.textContent = text.currentNode.textContent.replaceAll('Claude Code', c.agent);
    template.content.querySelector('.cc-top h2').textContent = c.title;
    template.content.querySelector('.cc-top small').textContent = `${c.project} · ${c.agent}`;
    return template.innerHTML + (open ? panel() : '');
  };
  VARIANTS.dConversationSwitcher = [
    ['Holding Option', () => { current = 0; order = [0, 1, 2, 3]; open = true; selected = 1; render(); }, () => open],
    ['Next conversation', () => { if (!open) begin(); step(1); }, () => false],
    ['Release Option', () => close(true), () => !open],
    ['Cancel', () => close(false), () => false],
    ['Pick a subagent', () => { current = 0; order = [0, 1, 2, 3]; open = true; selected = 5; render(); }, () => open && selected === 5],
    ['Return to pending steer', () => { selected = 0; close(true); }, () => !open && current === 0],
  ];
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dConversationSwitcher', 'Active conversation switcher', 'ConversationSwitcher']);
  document.addEventListener('keydown', event => {
    if (event.isComposing) return;
    if (event.altKey && event.key === 'Tab' && !event.ctrlKey && !event.metaKey) {
      event.preventDefault(); event.stopImmediatePropagation();
      if (!document.querySelector('.conversation-switcher-layer')) begin();
      step(event.shiftKey ? -1 : 1);
      return;
    }
    if (!document.querySelector('.conversation-switcher-layer')) return;
    if (['Escape', 'Enter', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Tab'].includes(event.key)) {
      event.preventDefault(); event.stopImmediatePropagation();
      if (event.key === 'Escape') close(false);
      else if (event.key === 'Enter') close(true);
      else step(['ArrowLeft', 'ArrowUp'].includes(event.key) || event.shiftKey ? -1 : 1);
    }
  }, true);
  document.addEventListener('keyup', event => {
    if (event.key === 'Alt' && document.querySelector('.conversation-switcher-layer')) close(true);
  }, true);
  document.addEventListener('click', event => {
    const choice = event.target.closest('[data-conversation-choice]');
    if (choice) { selected = Number(choice.dataset.conversationChoice); close(true); }
    else if (event.target.matches('[data-switcher-backdrop]')) close(false);
  });
  // Losing the window cancels rather than opening a conversation on return.
  window.addEventListener('blur', () => { if (document.querySelector('.conversation-switcher-layer')) close(false); });
  if (location.hash.slice(1) === 'dConversationSwitcher') S.screen = 'dConversationSwitcher';
  render();
})();
