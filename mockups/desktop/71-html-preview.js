// HTML source and its preview share a file tab.
(() => {
  let preview = false;
  D.dHtmlPreview = () => {
    const previous = S.d.ags;
    S.d.ags = 'filetext';
    const template = document.createElement('template');
    template.innerHTML = D.dAgentSession();
    S.d.ags = previous;
    const pane = template.content.querySelector('.ag-pane-content');
    const head = pane.querySelector('.ag-phead');
    pane.replaceChildren(head);
    const body = document.createElement('div');
    body.className = 'ag-pbody';
    if (preview) {
      const frame = document.createElement('iframe');
      frame.title = 'Preview of report.html';
      frame.setAttribute('sandbox', '');
      frame.style.cssText = 'width:100%;height:100%;border:0;background:white';
      frame.srcdoc = '<h1>Build report</h1><p>All checks passed.</p>';
      body.append(frame);
    } else {
      const code = document.createElement('pre');
      code.textContent = '<h1>Build report</h1>\n<p>All checks passed.</p>';
      body.append(code);
    }
    const footer = document.createElement('div');
    footer.className = 'ag-pacts';
    footer.innerHTML = `<span class="meta mono">report.html</span><span class="sp"></span><button class="dbtn" data-html-preview>${preview ? 'Source' : 'Preview'}</button><button class="dbtn" data-toast="Mentioned report.html">Mention</button><button class="dbtn" data-toast="Saved report.html">Save</button>`;
    pane.append(body, footer);
    return template.innerHTML;
  };
  document.addEventListener('click', event => {
    if (!event.target.closest('[data-html-preview]')) return;
    preview = !preview;
    render();
  });
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dHtmlPreview', 'Agent: HTML preview', 'HtmlPreview']);
  if (location.hash.slice(1) === 'dHtmlPreview') { S.screen = 'dHtmlPreview'; render(); }
})();
