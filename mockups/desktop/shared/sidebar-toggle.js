// Keep the same control available in both sidebar states.
(() => {
  let collapsed = false;
  const decorate = () => {
    document.querySelectorAll('.cc-win > .cc-side').forEach(side => {
      side.querySelectorAll('.cc-si').forEach(item => {
        const label = item.querySelector('.tt')?.textContent.trim();
        if (label) { item.title = label; item.setAttribute('aria-label', label); }
      });
      side.querySelectorAll(':scope > .cc-grp').forEach(group => {
        if (group.previousElementSibling?.matches('.cc-brand') || group.querySelector('[data-dgo="dWinSettings"]')) group.classList.add('cc-nav');
      });
      const brand = side.querySelector('.cc-brand');
      if (!brand || brand.querySelector('.cc-side-toggle')) return;
      const toggle = document.createElement('button');
      toggle.type = 'button';
      toggle.className = 'cc-side-toggle';
      toggle.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/></svg>';
      const update = () => {
        side.parentElement.classList.toggle('side-collapsed', collapsed);
        toggle.title = collapsed ? 'Expand sidebar' : 'Collapse sidebar';
        toggle.setAttribute('aria-label', toggle.title);
        toggle.setAttribute('aria-expanded', String(!collapsed));
      };
      toggle.addEventListener('click', () => { collapsed = !collapsed; update(); });
      brand.append(toggle);
      update();
    });
  };
  new MutationObserver(decorate).observe(document.getElementById('desk'), { childList: true, subtree: true });
  decorate();
})();
