const $ = selector => document.querySelector(selector);
const $$ = selector => [...document.querySelectorAll(selector)];
const directions = {
  grove: { title: 'A calm little companion.', description: 'Warm paper, forest green, and soft edges. A little editorial character in the full app; plain, readable type in the bubble.', palette: 'Paper #FAFBF5 · Forest #2C5740', tradeoff: 'My starting point: Grove. It already feels like Beep. Borrow Peach’s character, but keep the bubble quiet. Would you prefer a warmer background, or a sharper white?' },
  ink: { title: 'Less light. More focus.', description: 'Charcoal surfaces, lilac accents, and a compact rhythm. Typography does the work. A useful direction for a dedicated dark theme.', palette: 'Charcoal #202126 · Lilac #C1B9ED', tradeoff: 'After hours works well as Beep’s dark theme. It feels focused, but has less of the warmth of Grove. Should dark mode follow Android, or be a separate preference?' },
  pop: { title: 'A friend with a little spark.', description: 'Peach, cream, expressive shapes, and bolder typography. A more recognizable character without turning every answer into a cartoon.', palette: 'Cream #FFF7EA · Terracotta #B94325', tradeoff: 'Peach makes Beep more distinctive. I’d use its character in the icon and empty state, then quiet it down during a conversation. How playful should a daily assistant feel?' }
};
let theme = 'grove', moment = 'answer', compact = true;
function loadHash() {
 const params = new URLSearchParams(location.hash.slice(1));
 if (directions[params.get('theme')]) theme = params.get('theme');
 if (['welcome','answer','search','offline'].includes(params.get('state'))) moment = params.get('state');
 compact = params.get('compact') !== 'false';
}
loadHash();
function remember() { history.replaceState(null, '', `#theme=${theme}&state=${moment}&compact=${compact}`); }
function render() {
 const choice = directions[theme];
 $('.stage').dataset.theme = theme; $('.stage').dataset.state = moment;
 $('.stage').classList.toggle('roomy', !compact); $('#compact').checked = compact;
 $('#direction-title').textContent = choice.title; $('#direction-description').textContent = choice.description;
 $('#palette').textContent = choice.palette; $('#tradeoff').textContent = choice.tradeoff;
 $$('.direction').forEach(el => { const active = el.dataset.theme === theme; el.classList.toggle('selected', active); el.setAttribute('aria-pressed', active); });
 $$('.moments button').forEach(el => { const active = el.dataset.state === moment; el.classList.toggle('selected',active); el.setAttribute('aria-pressed',active); });
 $$('.chat-body').forEach(body => {
  // All templates are fixed illustrative copy, never model or user HTML.
  if (moment === 'welcome') body.innerHTML = `<div class="welcome-content"><div class="welcome-avatar" aria-hidden="true">••</div><h3>What’s on<br>your mind?</h3><p>Ask a question. Follow a thought.<br>I’m right here.</p><button class="starter" data-starter="search">Find something <span>↗</span></button><button class="starter" data-starter="answer">Think it through <span>↗</span></button></div>`;
  if (moment === 'answer') body.innerHTML = `<div class="user-message">Help me make a little room in my afternoon.</div><div class="assistant-label">BEEP</div><h3 class="answer-title">A little less, a little better.</h3><p class="answer-copy">Let’s keep it simple. Pick one thing that would make today feel good.</p><ol class="answer-list"><li><span>01</span><div><strong>Finish one small thing.</strong><br>Give it 25 uninterrupted minutes.</div></li><li><span>02</span><div><strong>Leave some breathing room.</strong><br>A walk. A coffee. No agenda.</div></li></ol><div class="copy-line">⧉ &nbsp; Copy reply</div>`;
  if (moment === 'search') body.innerHTML = `<div class="user-message">Find a good starting point for learning about urban gardens.</div><div class="assistant-label">BEEP</div><div class="searching"><i></i><span>Searching the web…</span></div><div class="search-lines" aria-hidden="true"><span></span><span></span><span></span></div><div class="sources"><span class="source">Looking for useful sources</span></div>`;
  if (moment === 'offline') body.innerHTML = `<div class="user-message">Let’s pick up where we left off.</div><div class="assistant-label">BEEP</div><h3 class="answer-title">A little out of reach.</h3><p class="offline-message">Beep can’t reach your server. Your conversation is still here.</p><button class="retry">Try again</button>`;
  body.scrollTop = 0;
 });
 $$('.starter').forEach(button => button.onclick = () => { moment = button.dataset.starter; update(); });
 $$('.retry').forEach(button => button.onclick = () => { moment = 'answer'; update(); $('#feedback').textContent = 'Preview: showing the reconnected state.'; });
 $$('.ready-dot').forEach(dot => { dot.hidden = moment === 'offline'; });
}
function update() { render(); remember(); }
$$('.direction').forEach(button => button.onclick = () => { theme = button.dataset.theme; update(); });
$$('.moments button').forEach(button => button.onclick = () => { moment = button.dataset.state; update(); });
$('#compact').onchange = event => { compact = event.target.checked; update(); };
$$('.new-chat').forEach(button => button.onclick = () => { moment = 'welcome'; closeMenu(); update(); });
function closeMenu() { $('.bubble-menu').hidden = true; $('.more').setAttribute('aria-expanded','false'); }
$('.more').onclick = () => { const open = $('.bubble-menu').hidden; $('.bubble-menu').hidden = !open; $('.more').setAttribute('aria-expanded',open); };
$('.close-bubble').onclick = () => { $('.bubble').hidden = true; closeMenu(); };
$('.floating-avatar').onclick = () => { $('.bubble').hidden = !$('.bubble').hidden; };
$('.expand').onclick = () => { $('.phone').scrollIntoView({behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth', block:'center'}); $('.phone').classList.add('focus-flash'); setTimeout(() => $('.phone').classList.remove('focus-flash'),1300); };
$$('.history').forEach(button => button.onclick = () => { closeMenu(); $('#history-dialog').showModal(); });
$('#close-history').onclick = () => $('#history-dialog').close();
$$('.sample-chat').forEach(button => button.onclick = () => { moment = 'answer'; update(); $('#history-dialog').close(); });
$$('.composer').forEach(form => form.onsubmit = event => {
 event.preventDefault(); const input = form.querySelector('input'); if (!input.value.trim()) return;
 const value = input.value.trim(); moment = 'answer'; update();
 $$('.chat-body').forEach(body => { body.replaceChildren(); const user = document.createElement('div'); user.className='user-message'; user.textContent=value; const reply = document.createElement('p'); reply.className='answer-copy'; reply.textContent='This is where Beep’s reply would appear. This playground previews the design; it doesn’t send messages to the assistant.'; body.append(user,reply); });
 $$('.composer input').forEach(input => input.value='');
});
try { $('#notes').value = localStorage.getItem('beep-design-notes') || ''; } catch {}
$('#notes').oninput = () => { try { localStorage.setItem('beep-design-notes',$('#notes').value); } catch { $('#feedback').textContent='Browser storage unavailable; copy your notes before closing.'; } };
$('#share').onclick = async () => {
 const text = `Beep direction: ${theme === 'ink' ? 'After hours' : theme === 'pop' ? 'Peach' : 'Grove'}\nBubble: ${compact?'compact':'roomy'}\n${location.href}\n\n${$('#notes').value}`;
 try {
  if (navigator.clipboard && window.isSecureContext) await navigator.clipboard.writeText(text);
  else { const field = document.createElement('textarea'); field.value=text; field.className='copy-buffer'; document.body.append(field); field.select(); const copied=document.execCommand('copy'); field.remove(); if(!copied) throw Error('Unavailable'); }
  $('#feedback').textContent='Copied. Paste it into our conversation.';
 } catch { $('#feedback').textContent='Copy the address bar link and your notes to share this direction.'; }
};
window.addEventListener('hashchange', () => { loadHash(); render(); });
document.addEventListener('keydown', event => { if(event.key === 'Escape') closeMenu(); });
render(); remember();
