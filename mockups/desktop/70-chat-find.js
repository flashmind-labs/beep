// Find inside a conversation, with wrapping next and previous matches.
(() => {
  let current = -1;
  function search(bar, step = 0) {
    const body = bar.nextElementSibling;
    body.querySelectorAll('mark.chat-match').forEach(mark => mark.replaceWith(...mark.childNodes));
    body.normalize();
    const input = bar.querySelector('input'), query = input.value, matches = [];
    let pattern;
    try {
      const regex = bar.querySelector('[data-find-regex]').getAttribute('aria-pressed') === 'true';
      pattern = query ? new RegExp(regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gimu') : null;
      bar.classList.remove('invalid'); input.removeAttribute('aria-invalid');
      bar.querySelector('output').removeAttribute('title');
    } catch (error) {
      bar.classList.add('invalid'); input.setAttribute('aria-invalid', 'true');
      bar.querySelector('output').textContent = 'Invalid regex';
      bar.querySelector('output').title = error.message;
      bar.querySelectorAll('[data-find-step]').forEach(button => button.disabled = true);
      current = -1;
      return;
    }
    if (pattern) {
      const walk = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
      const nodes = [];
      while (walk.nextNode()) nodes.push(walk.currentNode);
      for (const node of nodes) {
        const positions = [...node.textContent.matchAll(pattern)].filter(match => match[0].length);
        const marks = [];
        for (const match of positions.reverse()) {
          const at = match.index;
          const range = document.createRange();
          range.setStart(node, at); range.setEnd(node, at + match[0].length);
          const mark = document.createElement('mark'); mark.className = 'chat-match';
          range.surroundContents(mark); marks.unshift(mark);
        }
        matches.push(...marks);
      }
    }
    current = matches.length ? ((current + step) % matches.length + matches.length) % matches.length : -1;
    bar.querySelector('output').textContent = query ? (matches.length ? `${current + 1} of ${matches.length}` : 'No matches') : '';
    bar.querySelectorAll('[data-find-step]').forEach(button => button.disabled = !matches.length);
    if (current >= 0) { matches[current].classList.add('current'); matches[current].scrollIntoView({block:'center'}); }
  }
  function open() {
    const body = document.querySelector('#desk .dbody');
    if (!body) return;
    let bar = body.previousElementSibling;
    if (!bar?.matches('.chat-find')) {
      bar = document.createElement('div'); bar.className = 'chat-find'; bar.setAttribute('role', 'search');
      bar.innerHTML = '<input placeholder="Find in chat" aria-label="Find in chat" aria-describedby="chat-find-status"><button class="chat-find-regex" data-find-regex aria-label="Use regular expression" title="Use regular expression" aria-pressed="false">.*</button><output id="chat-find-status" aria-live="polite"></output><button data-find-step="-1" aria-label="Previous match" title="Previous match (Shift+Enter)"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m18 15-6-6-6 6"/></svg></button><button data-find-step="1" aria-label="Next match" title="Next match (Enter)"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 9 6 6 6-6"/></svg></button><button data-find-close aria-label="Close search" title="Close search (Esc)"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 6 12 12M6 18 18 6"/></svg></button>';
      body.before(bar); current = 0;
      search(bar);
      bar.querySelector('input').oninput = () => { current = 0; search(bar); };
      bar.onclick = event => {
        const button = event.target.closest('button');
        if (button?.hasAttribute('data-find-regex')) {
          button.setAttribute('aria-pressed', String(button.getAttribute('aria-pressed') !== 'true'));
          current = 0; search(bar); bar.querySelector('input').focus();
        }
        if (button?.hasAttribute('data-find-step')) search(bar, Number(button.dataset.findStep));
        if (button?.hasAttribute('data-find-close')) close(bar);
      };
    }
    bar.querySelector('input').focus(); bar.querySelector('input').select();
  }
  function close(bar) {
    bar.nextElementSibling.querySelectorAll('mark.chat-match').forEach(mark => mark.replaceWith(...mark.childNodes));
    bar.remove();
  }
  document.addEventListener('keydown', event => {
    const bar = document.querySelector('#desk .chat-find');
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') { event.preventDefault(); event.stopImmediatePropagation(); open(); }
    else if (bar && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'g') { event.preventDefault(); event.stopImmediatePropagation(); search(bar, event.shiftKey ? -1 : 1); }
    else if (bar && event.key === 'Escape') { event.preventDefault(); event.stopImmediatePropagation(); close(bar); }
    else if (bar && event.target.matches('.chat-find input') && event.key === 'Enter' && !event.isComposing) { event.preventDefault(); event.stopImmediatePropagation(); search(bar, event.shiftKey ? -1 : 1); }
  }, true);
  D.dChatFind = () => {
    setTimeout(() => {
      if (S.screen !== 'dChatFind') return;
      open();
      const bar = document.querySelector('#desk .chat-find');
      const mode = S.d.chatFindMode || 'matches';
      bar.querySelector('input').value = mode === 'empty' ? '' : mode === 'missing' ? 'unmatched phrase' : mode === 'regex' ? 'build|test' : mode === 'invalid' ? '[' : 'build';
      bar.querySelector('[data-find-regex]').setAttribute('aria-pressed', String(mode === 'regex' || mode === 'invalid'));
      current = mode === 'next' ? 1 : 0;
      search(bar);
      bar.querySelector('input').select();
    }, 0);
    const mode = S.d.ags;
    S.d.ags = 'toolcalls';
    try { return D.dAgentSession(); } finally { S.d.ags = mode; }
  };
  VARIANTS.dChatFind = [
    ['Matches', 'matches'], ['Next match', 'next'], ['No matches', 'missing'], ['Empty query', 'empty'], ['Regex', 'regex'], ['Invalid regex', 'invalid'],
  ].map(([label, mode]) => [label, () => { S.d.chatFindMode = mode; render(); }, () => (S.d.chatFindMode || 'matches') === mode]);
  DESK_SCREENS.find(group => group.group === 'Command center').items.push(['dChatFind', 'Chat: find (Cmd+F)', 'ChatFind']);
  if (location.hash === '#dChatFind') { S.screen = 'dChatFind'; render(); }
})();
