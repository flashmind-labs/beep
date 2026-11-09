// Gallery navigation is separate from the app sidebar inside each mock screen.
(() => {
  const index = document.getElementById('index');
  const features = [
    ['Home', /Home|Notifications|Palette|Shortcuts/],
    ['Projects and files', /Project|Import|Folder|Instructions|Sync|HtmlPreview/],
    ['Coding agents', /Agent|Providers|ConversationSwitcher|ChatFind/],
    ['Machines', /Machine|Ticket/],
    ['Terminals', /Terminal/],
    ['Settings and help', /Settings|Appearance|SetApp|SetHarness|ApiKeys|Report/],
  ];
  const appGroups = DESK_SCREENS.filter(group => ['Current app', 'Preview: Command center', 'Preview: Local first'].includes(group.group));
  const appItems = appGroups.flatMap(group => group.items);
  const used = new Set();
  const children = features.map(([label, pattern]) => {
    const items = appItems.filter(([id]) => !used.has(id) && pattern.test(id));
    items.forEach(([id]) => used.add(id));
    return { label, items };
  });
  children.push({ label: 'Chats and assistant', items: appItems.filter(([id]) => !used.has(id)) });
  const tree = [
    { label: 'App screens', children: children.filter(group => group.items.length) },
    { label: 'Design ideas', children: DESK_SCREENS.filter(group => group.group.startsWith('Ideas')).map(group => ({ label: group.group, items: group.items })) },
    { label: 'Other previews', children: DESK_SCREENS.filter(group => !appGroups.includes(group) && !group.group.startsWith('Ideas')).map(group => ({ label: group.group.replace('Preview: ', ''), items: group.items })) },
  ];
  let expanded = {};
  try { expanded = JSON.parse(localStorage.getItem('beep-mockup-menu') || '{}') || {}; } catch { /* Use the default sections when storage is unavailable. */ }
  let query = '';
  let lastScreen = null;
  const count = branch => branch.items ? branch.items.length : branch.children.reduce((sum, child) => sum + count(child), 0);
  const contains = branch => branch.items ? branch.items.some(([id]) => id === S.screen) : branch.children.some(contains);
  const matching = (items, label) => items.filter(([id, name]) => `${label} ${id} ${name}`.toLowerCase().includes(query.trim().toLowerCase()));
  function branchHtml(branch, path, parentLabel = '') {
    const key = path.join('/');
    const label = `${parentLabel} ${branch.label}`;
    const items = branch.items ? matching(branch.items, label) : null;
    const body = items ? items.map(([id, name]) => `<button data-screen="${esc(id)}" aria-current="${S.screen === id}" title="${esc(id)}">${esc(name)}</button>`).join('')
      : branch.children.map((child, at) => branchHtml(child, [...path, at], label)).join('');
    if (!body) return '';
    const open = query.trim() || expanded[key];
    return `<details data-menu-branch="${key}"${open ? ' open' : ''}><summary><span>${esc(branch.label)}</span><small>${query.trim() && items ? items.length : count(branch)}</small></summary><div class="screen-children">${body}</div></details>`;
  }
  renderIndex = () => {
    if (lastScreen !== S.screen) {
      tree.forEach((branch, root) => {
        if (!contains(branch)) return;
        expanded[String(root)] = true;
        branch.children.forEach((child, at) => { if (contains(child)) expanded[`${root}/${at}`] = true; });
      });
      lastScreen = S.screen;
    }
    const focused = document.activeElement?.id === 'screen-search';
    const caret = focused ? document.activeElement.selectionStart : null;
    const scroll = index.scrollTop;
    const body = tree.map((branch, at) => branchHtml(branch, [at])).join('');
    index.innerHTML = `<div class="screen-menu-head"><strong>Mockup screens</strong><label class="screen-search"><span class="screen-search-label">Find a screen</span><input id="screen-search" type="search" placeholder="Search screens" autocomplete="off" value="${esc(query)}"></label><div class="screen-menu-tools"><span>${appItems.length + tree.slice(1).reduce((sum, branch) => sum + count(branch), 0)} screens</span><button type="button" data-menu-collapse>Collapse all</button></div></div><div class="screen-tree">${body || '<p role="status">No screens match. Try another search.</p>'}</div>`;
    index.scrollTop = scroll;
    if (focused) {
      const input = document.getElementById('screen-search');
      input.focus();
      if (caret !== null) input.setSelectionRange(caret, caret);
    }
  };
  index.addEventListener('input', event => {
    if (event.target.id !== 'screen-search') return;
    query = event.target.value;
    renderIndex();
  });
  index.addEventListener('keydown', event => {
    if (event.target.id !== 'screen-search') return;
    if (event.key === 'Escape') { event.stopPropagation(); query = ''; renderIndex(); }
    if (event.key === 'Enter') { const first = index.querySelector('[data-screen]'); if (first) { event.preventDefault(); go(first.dataset.screen); } }
  });
  index.addEventListener('toggle', event => {
    if (!event.target.matches('details[data-menu-branch]') || !event.target.isConnected || query.trim()) return;
    expanded[event.target.dataset.menuBranch] = event.target.open;
    try { localStorage.setItem('beep-mockup-menu', JSON.stringify(expanded)); } catch { /* Navigation works without storage. */ }
  }, true);
  index.addEventListener('click', event => {
    if (!event.target.closest('[data-menu-collapse]')) return;
    expanded = {}; query = '';
    try { localStorage.setItem('beep-mockup-menu', '{}'); } catch { /* Navigation works without storage. */ }
    renderIndex();
  });
  renderIndex();
})();
