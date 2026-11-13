// A quoted reply with the same copy control used by the app.
(() => {
  const COPY = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/></svg>';
  D.dQuoteCopy = () => {
    const template = document.createElement('template');
    template.innerHTML = D.dAgentSession();
    const body = template.content.querySelector('.dbody');
    if (body) body.innerHTML = `<div class="dcoderen agent md"><p>Paste this:</p>
      <blockquote class="quote-copy-example">
        <p>Check the upload retry behavior.</p>
        <p>Run the tests, then disconnect the network during an upload. Confirm that the upload resumes when the connection returns.</p>
        <ul><li>Keep completed chunks.</li><li>Retry only the unfinished work.</li><li>Report a clear error if retries run out.</li></ul>
        <p>Share the result and any failing test.</p>
      </blockquote><button type="button" class="dcopy" data-quote-copy aria-label="Copy quote" title="Copy quote">${COPY}</button><p>The button copies only the quote, keeping its paragraphs and list.</p></div>`;
    return template.innerHTML;
  };
  DESK_SCREENS.push({ group: 'Quote copy', items: [['dQuoteCopy', 'Copy quoted text', 'Quoted reply with a copy button']] });
  document.addEventListener('click', async event => {
    const button = event.target.closest('[data-quote-copy]');
    if (!button) return;
    const quote = button.previousElementSibling.cloneNode(true);
    quote.querySelectorAll('button').forEach(control => control.remove());
    quote.querySelectorAll('p, li').forEach(block => block.append('\n\n'));
    try {
      await navigator.clipboard.writeText(quote.textContent.trim());
      button.innerHTML = DI.check;
      button.setAttribute('aria-label', 'Copied');
      setTimeout(() => { button.innerHTML = COPY; button.setAttribute('aria-label', 'Copy quote'); }, 1200);
    } catch {
      button.textContent = 'Copy failed';
    }
  });
})();
