// A quoted reply with the same copy control used by the app.
(() => {
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
      </blockquote><button type="button" class="dcopy" data-quote-copy aria-label="Copy quote" title="Copy quote">${D_COPY}</button><p>The button copies only the quote, keeping its paragraphs and list.</p></div>`;
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
      setTimeout(() => { button.innerHTML = D_COPY; button.setAttribute('aria-label', 'Copy quote'); }, 1200);
    } catch {
      button.textContent = 'Copy failed';
    }
  });
})();
