// File references in replies share the local file actions of the Files tab.
(() => {
  D.dAgentFileLinks = () => {
    const previous = S.d.ags;
    S.d.ags = 'idle';
    const template = document.createElement('template');
    template.innerHTML = D.dAgentSession();
    S.d.ags = previous;
    template.content.querySelector('.dcoderlog').innerHTML = dYou('Update the desktop mockup')
      + '<div class="dcoderen agent md">Implemented the <a href="mockups/desktop/index.html" data-file-link>mockup</a>: normal conversation, thoughts, tools, and composer.</div>';
    return template.innerHTML;
  };
  document.addEventListener('contextmenu', event => {
    const link = event.target.closest('[data-file-link]');
    if (!link) return;
    event.preventDefault();
    document.querySelector('[data-file-link-menu]')?.remove();
    const menu = document.createElement('div');
    menu.className = 'ag-ctx';
    menu.dataset.fileLinkMenu = '';
    menu.setAttribute('role', 'menu');
    menu.setAttribute('aria-label', link.getAttribute('href'));
    Object.assign(menu.style, { position: 'fixed', left: `${event.clientX}px`, top: `${event.clientY}px`, zIndex: '9999' });
    const fill = items => {
      menu.replaceChildren();
      for (const [label, action] of items) {
        const button = document.createElement('button');
        button.textContent = label;
        button.setAttribute('role', 'menuitem');
        button.onclick = action;
        menu.append(button);
      }
      menu.querySelector('button')?.focus();
    };
    const close = () => { menu.remove(); document.removeEventListener('mousedown', away); };
    const away = e => { if (!menu.contains(e.target)) close(); };
    const done = text => () => { close(); toast(text); };
    fill([
      ['Open in Beep', done('Opened in Beep')],
      ['Copy path', done('Copied mockups/desktop/index.html')],
      ['Open in default app', done('Opened in the default browser')],
      ['Open With', () => fill(['Google Chrome', 'Safari (default)', 'TextEdit'].map(app => [app, done(`Opened in ${app}`)]))],
      ['Reveal in Finder', done('Revealed in Finder')],
    ]);
    document.querySelector('.mac').append(menu);
    const rect = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(event.clientX, innerWidth - rect.width - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(event.clientY, innerHeight - rect.height - 4))}px`;
    menu.querySelector('button')?.focus();
    document.addEventListener('mousedown', away);
    menu.onkeydown = e => {
      if (e.key === 'Escape') { close(); link.focus(); }
      else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        const buttons = [...menu.querySelectorAll('button')];
        buttons[(buttons.indexOf(document.activeElement) + (e.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length].focus();
      } else return;
      e.preventDefault();
      e.stopPropagation();
    };
  });
  document.addEventListener('click', event => {
    if (event.target.closest('[data-file-link]')) { event.preventDefault(); toast('Opened in Beep'); }
  });
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dAgentFileLinks', 'Agent: file references', 'AgentFileLinks']);
  if (location.hash.slice(1) === 'dAgentFileLinks') { S.screen = 'dAgentFileLinks'; render(); }
})();
