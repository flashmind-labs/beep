import { readEvents, acceptEvent, uuid } from './protocol.mjs';
const $ = id => document.getElementById(id);
const storage = { get(key) { try { return sessionStorage.getItem(key); } catch { return null; } }, set(key, value) { try { sessionStorage.setItem(key, value); } catch {} }, clear() { try { sessionStorage.removeItem('beep-token'); sessionStorage.removeItem('beep-chat'); } catch {} } };
let token = storage.get('beep-token') || '';
let testing = false;
let state;
try { state = JSON.parse(storage.get('beep-chat')); } catch {}
if (!state || !Array.isArray(state.chats) || (!token && storage.get('beep-mode') !== 'testing')) state = { chats: [], selected: null };
for (const chat of state.chats) chat.loading = false;
let connected = false, working = false, generation = 0, controller;
const save = () => storage.set('beep-chat', JSON.stringify(state));
const current = () => state.chats.find(c => c.id === state.selected);

// --- Views ---
let currentView = 'chat';
function showView(name) {
  currentView = name;
  document.querySelectorAll('.view').forEach(v => v.classList.toggle('active', v.id === `view-${name}`));
  document.querySelectorAll('.sidebar-link').forEach(l => l.classList.toggle('active', l.id === `nav-${name}`));
  if (name === 'memories') loadMemories();
  if (name === 'crons') loadCrons();
}
$('nav-memories').onclick = () => showView('memories');
$('nav-crons').onclick = () => showView('crons');
document.querySelectorAll('.back-btn').forEach(btn => btn.onclick = () => showView(btn.dataset.view));

// --- Markdown rendering ---
function appendInline(element, text) {
  const lines = text.split('\n');
  for (let li = 0; li < lines.length; li++) {
    if (li > 0) element.append(document.createElement('br'));
    const line = lines[li];
    const pattern = /(\*\*(.+?)\*\*|\*([^*\n]+?)\*|`([^`\n]+)`|\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s<>]+))/g;
    let offset = 0;
    for (const match of line.matchAll(pattern)) {
      if (match.index > offset) element.append(document.createTextNode(line.slice(offset, match.index)));
      if (match[2]) { const el = document.createElement('strong'); el.textContent = match[2]; element.append(el); }
      else if (match[3]) { const el = document.createElement('em'); el.textContent = match[3]; element.append(el); }
      else if (match[4]) { const el = document.createElement('code'); el.textContent = match[4]; element.append(el); }
      else if (match[5]) { const a = document.createElement('a'); a.href = match[6]; a.textContent = match[5]; a.target = '_blank'; a.rel = 'noopener noreferrer'; element.append(a); }
      else if (match[7]) { const url = match[7].replace(/[.,;:!?)]+$/, ''); const a = document.createElement('a'); a.href = url; a.textContent = url; a.target = '_blank'; a.rel = 'noopener noreferrer'; element.append(a); if (match[0].length > url.length) element.append(document.createTextNode(match[0].slice(url.length))); }
      offset = match.index + match[0].length;
    }
    if (offset < line.length) element.append(document.createTextNode(line.slice(offset)));
  }
}
function renderMarkdown(container, text) {
  if (!text) return;
  const lines = text.split('\n');
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith('```')) {
      const lang = line.slice(3).trim(); const codeLines = []; i++;
      while (i < lines.length && !lines[i].startsWith('```')) { codeLines.push(lines[i]); i++; }
      if (i < lines.length) i++;
      const pre = document.createElement('pre'); const code = document.createElement('code');
      code.textContent = codeLines.join('\n'); if (lang) pre.dataset.lang = lang;
      pre.append(code); container.append(pre); continue;
    }
    const hm = line.match(/^(#{1,4})\s+(.+)/);
    if (hm) { const h = document.createElement('h' + Math.min(hm[1].length + 2, 6)); appendInline(h, hm[2]); container.append(h); i++; continue; }
    if (/^[-*+]\s/.test(line)) {
      const ul = document.createElement('ul');
      while (i < lines.length && /^[-*+]\s/.test(lines[i])) { const li = document.createElement('li'); appendInline(li, lines[i].replace(/^[-*+]\s+/, '')); ul.append(li); i++; }
      container.append(ul); continue;
    }
    if (/^\d+[.)]\s/.test(line)) {
      const ol = document.createElement('ol');
      while (i < lines.length && /^\d+[.)]\s/.test(lines[i])) { const li = document.createElement('li'); appendInline(li, lines[i].replace(/^\d+[.)]\s+/, '')); ol.append(li); i++; }
      container.append(ol); continue;
    }
    if (line.startsWith('>')) {
      const bq = document.createElement('blockquote');
      while (i < lines.length && lines[i].startsWith('>')) { const p = document.createElement('p'); appendInline(p, lines[i].replace(/^>\s*/, '')); bq.append(p); i++; }
      container.append(bq); continue;
    }
    if (/^[-*_]{3,}\s*$/.test(line)) { container.append(document.createElement('hr')); i++; continue; }
    if (!line.trim()) { i++; continue; }
    const pLines = [];
    while (i < lines.length && lines[i].trim() && !lines[i].startsWith('```') && !/^#{1,4}\s/.test(lines[i]) && !/^[-*+]\s/.test(lines[i]) && !/^\d+[.)]\s/.test(lines[i]) && !lines[i].startsWith('>') && !/^[-*_]{3,}\s*$/.test(lines[i])) { pLines.push(lines[i]); i++; }
    const p = document.createElement('p'); appendInline(p, pLines.join('\n')); container.append(p);
  }
}

// --- Chat ---
function newChat() {
  if (working) return;
  const chat = { id: uuid(), title: 'New chat', messages: [], pending: null };
  state.chats.unshift(chat); state.selected = chat.id; save(); showView('chat'); render(); $('message').focus();
}
function render() {
  const chat = current();
  $('chat-title').textContent = chat?.title || 'New chat';
  $('welcome').hidden = Boolean(chat?.messages.length);
  const box = $('messages');
  const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 130;
  box.replaceChildren();
  const msgs = chat?.messages || [];
  for (let idx = 0; idx < msgs.length; idx++) {
    const message = msgs[idx]; const div = document.createElement('div'); div.className = `message ${message.role}`;
    div.style.animationDelay = `${Math.min(idx * 30, 200)}ms`;
    const text = message.text || (message.role === 'assistant' ? 'Thinking...' : '');
    if (message.role === 'assistant') {
      renderMarkdown(div, text);
      const isActive = chat?.pending && idx === chat.pending.answerIndex;
      if (message.text && !isActive) { const btn = document.createElement('button'); btn.className = 'copy-btn'; btn.textContent = 'Copy'; btn.onclick = () => { navigator.clipboard.writeText(message.text).then(() => { btn.textContent = 'Copied'; setTimeout(() => btn.textContent = 'Copy', 1500); }).catch(() => {}); }; div.append(btn); }
    } else { div.textContent = text; }
    box.append(div);
  }
  if (nearBottom) box.scrollTop = box.scrollHeight;
  $('conversations').replaceChildren();
  for (const c of state.chats) {
    const button = document.createElement('button'); button.textContent = c.title; button.className = c.id === state.selected ? 'selected' : '';
    button.disabled = working; button.onclick = () => { state.selected = c.id; save(); notice(''); showView('chat'); render(); if (connected && c.pending) resume(c); else if (connected && c.serverSaved) hydrate(c); };
    $('conversations').append(button);
  }
  $('send').hidden = working; $('stop').hidden = !working || !chat?.pending?.runId;
  $('send').disabled = working || Boolean(chat?.pending) || Boolean(chat?.loading); $('message').disabled = working || Boolean(chat?.pending) || Boolean(chat?.loading);
  $('new-chat').disabled = working; $('mobile-new').disabled = working; $('connect-submit').disabled = working; $('token').disabled = working;
}
function connection(value, label) { connected = value; $('connection-label').textContent = label; $('status-dot').classList.toggle('connected', value); }
function notice(text, retry) {
  const box = $('notice'); box.replaceChildren(); box.hidden = !text;
  if (text) box.append(document.createTextNode(text));
  if (retry) { const button = document.createElement('button'); button.textContent = 'Retry'; button.onclick = retry; box.append(button); }
}
class ApiError extends Error { constructor(status, message) { super(message); this.status = status; } }
async function api(path, { method = 'GET', data, signal } = {}) {
  const response = await fetch(path, { method, cache: 'no-store', credentials: 'omit', headers: { ...(token && !testing ? { Authorization: `Bearer ${token}` } : {}), ...(data ? { 'Content-Type': 'application/json' } : {}) }, body: data ? JSON.stringify(data) : undefined, signal });
  if (!response.ok) {
    let detail; try { detail = (await response.json()).error; } catch {}
    throw new ApiError(response.status, response.status === 401 ? 'Token rejected. Check connection settings.' : detail || `Server returned ${response.status}.`);
  }
  return response;
}
async function hydrate(chat) {
  if (chat.pending || chat.loading) return;
  const owner = generation; chat.loading = true; render();
  try {
    const history = await (await api(`/v1/sessions/${encodeURIComponent(chat.id)}`, { signal: AbortSignal.timeout(10000) })).json();
    if (owner !== generation || chat.pending) return;
    chat.messages = history.entries.filter(entry => ['user', 'assistant'].includes(entry.role) && typeof entry.content === 'string' && entry.content.length).map(entry => ({ role: entry.role, text: entry.content }));
  } catch (error) { if (owner === generation) notice(`Couldn't load conversation: ${error.message}`, () => hydrate(chat)); }
  finally { chat.loading = false; if (owner === generation) { save(); render(); } }
}
async function discover() {
  const owner = generation;
  try {
    const result = await (await api('/v1/sessions', { signal: AbortSignal.timeout(10000) })).json();
    if (owner !== generation) return;
    for (const summary of result.sessions) {
      let chat = state.chats.find(item => item.id === summary.id);
      if (!chat) { chat = { id: summary.id, title: summary.title, messages: [], pending: null }; state.chats.push(chat); }
      chat.serverSaved = true;
      if (!chat.pending) chat.title = summary.title || 'Conversation';
    }
    if (!current() && state.chats.length) state.selected = state.chats[0].id;
    if (current()?.serverSaved && !current().pending) await hydrate(current());
    save(); render();
  } catch (error) { if (owner === generation) notice(`Connected, but couldn't load conversations: ${error.message}`, discover); }
}
async function connect(nextToken) {
  if (working) { $('connect-error').textContent = 'Wait for the current reply first.'; return; }
  $('connect-submit').disabled = true; $('connect-error').textContent = '';
  const previousToken = token; const connectGeneration = generation; token = nextToken;
  try {
    await api('/v1/health', { signal: AbortSignal.timeout(10000) });
    if (connectGeneration !== generation) return;
    if (previousToken !== token) { generation++; controller?.abort(); working = false; state = { chats: [], selected: null }; storage.clear(); $('activity').textContent = ''; notice(''); }
    storage.set('beep-token', token); connection(true, testing ? 'Testing' : 'Connected'); $('connection-dialog').close(); $('token').value = ''; $('disconnect').hidden = false;
    await discover();
    if (!current()) newChat(); else render(); save();
    if (current()?.pending) resume(current());
  } catch (error) { if (connectGeneration !== generation) return; token = previousToken; $('connect-error').textContent = error instanceof ApiError ? error.message : 'Cannot reach server.'; connection(false, 'Disconnected'); }
  finally { $('connect-submit').disabled = false; }
}
async function resume(chat) {
  if (working || !chat.pending) return;
  const thisGeneration = generation; controller = new AbortController(); const signal = controller.signal;
  const pending = chat.pending; working = true; notice(''); render();
  try {
    if (!pending.runId) {
      $('activity').textContent = 'Sending...';
      const run = await (await api('/v1/runs', { method: 'POST', data: { session_id: chat.id, request_id: pending.requestId, message: pending.message }, signal })).json();
      pending.runId = run.id; save(); render();
    }
    let terminal = false, attempts = 0;
    while (!terminal && thisGeneration === generation) {
      try {
        $('activity').textContent = attempts ? 'Reconnecting...' : 'Thinking...';
        const response = await api(`/v1/runs/${pending.runId}/events?after=${pending.after}`, { signal });
        await readEvents(response.body, event => {
          if (thisGeneration !== generation || !acceptEvent(pending, event)) return;
          const answer = chat.messages[pending.answerIndex];
          if (event.type === 'text_delta') { answer.text += event.delta; $('activity').textContent = 'Replying...'; }
          if (event.type === 'tool_started') $('activity').textContent = event.description || `Using ${event.name.replaceAll('_', ' ')}...`;
          if (event.type === 'tool_result') $('activity').textContent = event.success ? 'Done. Thinking...' : 'Tool problem...';
          if (event.type === 'error') { notice(event.message); answer.text ||= event.message; }
          if (event.type === 'run_finished') { terminal = true; finish(chat, event.status); }
          save(); render();
        });
        if (!terminal) {
          const run = await (await api(`/v1/runs/${pending.runId}`, { signal })).json();
          if (run.status !== 'running') {
            const tail = await api(`/v1/runs/${pending.runId}/events?after=${pending.after}`, { signal });
            await readEvents(tail.body, event => {
              if (!acceptEvent(pending, event)) return;
              if (event.type === 'text_delta') chat.messages[pending.answerIndex].text += event.delta;
            });
            terminal = true; finish(chat, run.status); save(); render();
          }
        }
        attempts = 0;
      } catch (error) {
        if (signal.aborted || error instanceof ApiError || ++attempts > 3) throw error;
        await new Promise(resolve => setTimeout(resolve, attempts * 1000));
      }
    }
  } catch (error) {
    if (thisGeneration !== generation) return;
    if (error instanceof ApiError && !pending.runId && [400, 401, 404, 409, 413].includes(error.status)) {
      chat.messages.splice(pending.answerIndex - 1, 2); chat.pending = null; $('message').value = pending.message;
    }
    if (error.status === 401) connection(false, 'Check token');
    notice(error instanceof ApiError ? error.message : 'Connection interrupted. Retry to resume.', chat.pending ? () => resume(chat) : null);
  } finally {
    if (thisGeneration === generation) { working = false; $('activity').textContent = ''; save(); render(); }
  }
}
function finish(chat, status) {
  const answer = chat.messages[chat.pending.answerIndex];
  if (!answer.text) answer.text = status === 'completed' ? 'Done.' : `Run ${status.replaceAll('_', ' ')}.`;
  if (status !== 'completed') notice(`Run ${status.replaceAll('_', ' ')}.`);
  chat.pending = null; chat.serverSaved = true;
}
$('composer').onsubmit = event => {
  event.preventDefault(); if (working || current()?.pending || current()?.loading) return;
  if (!connected) { $('connection-dialog').showModal(); return; }
  const message = $('message').value.trim(); if (!message) return;
  if (new TextEncoder().encode(message).length > 32768) { notice('Message too long (max 32 KiB).'); return; }
  if (!current()) newChat(); const chat = current();
  if (!chat.messages.length) chat.title = message.slice(0, 50);
  chat.messages.push({ role: 'user', text: message }, { role: 'assistant', text: '' });
  chat.pending = { requestId: uuid(), message, runId: null, after: 0, answerIndex: chat.messages.length - 1 };
  $('message').value = ''; $('message').style.height = ''; save(); resume(chat);
};
$('message').onkeydown = event => { if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); $('composer').requestSubmit(); } };
$('message').oninput = () => { $('message').style.height = 'auto'; $('message').style.height = `${Math.min($('message').scrollHeight, 140)}px`; };
$('stop').onclick = async () => {
  const id = current()?.pending?.runId; if (!id) return;
  try { await api(`/v1/runs/${id}/cancel`, { method: 'POST', signal: AbortSignal.timeout(10000) }); $('activity').textContent = 'Stopping...'; } catch (error) { notice(error.message); }
};
$('new-chat').onclick = newChat; $('mobile-new').onclick = newChat;
$('mobile-drawer').onclick = () => document.querySelector('.sidebar').classList.toggle('open');
$('connection-button').onclick = () => { if (testing) { if (!connected) boot(); return; } $('disconnect').hidden = !token; $('connection-dialog').showModal(); };
$('close-dialog').onclick = () => $('connection-dialog').close();
$('connect-form').onsubmit = event => { event.preventDefault(); connect($('token').value.trim()); };
$('disconnect').onclick = () => { generation++; controller?.abort(); working = false; token = ''; storage.clear(); state = { chats: [], selected: null }; connection(false, 'Connect'); $('activity').textContent = ''; notice(''); $('disconnect').hidden = true; render(); };
for (const button of document.querySelectorAll('[data-prompt]')) button.onclick = () => { $('message').value = button.dataset.prompt; $('message').focus(); };

// --- Memories ---
async function loadMemories() {
  if (!connected) return;
  try {
    const result = await (await api('/v1/memories', { signal: AbortSignal.timeout(10000) })).json();
    const list = $('memory-list'); list.replaceChildren();
    const query = $('memory-search').value.toLowerCase();
    const filtered = result.memories.filter(m => !query || m.key.toLowerCase().includes(query) || m.content.toLowerCase().includes(query));
    $('memory-count').textContent = `${filtered.length} memor${filtered.length === 1 ? 'y' : 'ies'}`;
    $('memory-empty').hidden = filtered.length > 0;
    for (const mem of filtered) {
      const card = document.createElement('div'); card.className = 'item-card';
      card.innerHTML = ''; // safe: we use textContent below
      const body = document.createElement('div'); body.className = 'item-body';
      const key = document.createElement('p'); key.className = 'item-key'; key.textContent = mem.key;
      const value = document.createElement('p'); value.className = 'item-value'; value.textContent = mem.content;
      body.append(key, value);
      const actions = document.createElement('div'); actions.className = 'item-actions';
      const del = document.createElement('button'); del.className = 'delete-btn'; del.textContent = '\u00d7'; del.title = 'Delete';
      del.onclick = async () => { try { await api('/v1/memories', { method: 'DELETE', data: { key: mem.key } }); loadMemories(); } catch (e) { notice(e.message); } };
      actions.append(del);
      card.append(body, actions); list.append(card);
    }
  } catch (error) { $('memory-list').replaceChildren(); $('memory-count').textContent = error.message; }
}
$('memory-search').oninput = loadMemories;

// --- Crons ---
async function loadCrons() {
  if (!connected) return;
  try {
    const result = await (await api('/v1/crons', { signal: AbortSignal.timeout(10000) })).json();
    const list = $('cron-list'); list.replaceChildren();
    $('cron-count').textContent = `${result.crons.length} automation${result.crons.length === 1 ? '' : 's'}`;
    $('cron-empty').hidden = result.crons.length > 0;
    for (const cron of result.crons) {
      const card = document.createElement('div'); card.className = 'item-card';
      const body = document.createElement('div'); body.className = 'item-body';
      const key = document.createElement('p'); key.className = 'item-key'; key.textContent = cron.label;
      const value = document.createElement('p'); value.className = 'item-value'; value.textContent = cron.prompt;
      const meta = document.createElement('p'); meta.className = 'item-meta'; meta.textContent = `Schedule: ${cron.schedule}`;
      body.append(key, value, meta);
      const actions = document.createElement('div'); actions.className = 'item-actions';
      const toggle = document.createElement('button'); toggle.className = `toggle-btn${cron.enabled ? ' enabled' : ''}`;
      toggle.textContent = cron.enabled ? 'On' : 'Off';
      toggle.onclick = async () => { try { await api(`/v1/crons/${cron.id}`, { method: 'PATCH', data: { enabled: !cron.enabled } }); loadCrons(); } catch (e) { notice(e.message); } };
      const del = document.createElement('button'); del.className = 'delete-btn'; del.textContent = '\u00d7'; del.title = 'Delete';
      del.onclick = async () => { try { await api(`/v1/crons/${cron.id}`, { method: 'DELETE' }); loadCrons(); } catch (e) { notice(e.message); } };
      actions.append(toggle, del);
      card.append(body, actions); list.append(card);
    }
  } catch (error) { $('cron-list').replaceChildren(); $('cron-count').textContent = error.message; }
}
$('new-cron').onclick = () => $('cron-dialog').showModal();
$('cron-cancel').onclick = () => $('cron-dialog').close();
$('cron-form').onsubmit = async event => {
  event.preventDefault(); $('cron-error').textContent = '';
  try {
    await api('/v1/crons', { method: 'POST', data: { label: $('cron-label').value.trim(), prompt: $('cron-prompt').value.trim(), schedule: $('cron-schedule').value.trim() } });
    $('cron-dialog').close(); $('cron-label').value = ''; $('cron-schedule').value = ''; $('cron-prompt').value = ''; loadCrons();
  } catch (error) { $('cron-error').textContent = error.message; }
};

// --- Boot ---
async function boot() {
  connection(false, 'Connecting...');
  try {
    const response = await fetch('/v1/auth', { cache: 'no-store', credentials: 'omit', signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error('Cannot read auth mode.');
    const config = await response.json();
    if (!['testing', 'token'].includes(config.mode)) throw new Error('Unknown auth mode.');
    if (storage.get('beep-mode') !== config.mode) { storage.clear(); token = ''; state = { chats: [], selected: null }; }
    storage.set('beep-mode', config.mode);
    testing = config.mode === 'testing';
    if (testing) {
      token = ''; $('connection-dialog').close();
      await connect('');
      if (!connected) notice('Cannot reach test server.', boot);
    } else if (token) await connect(token);
    else { connection(false, 'Connect'); $('connection-dialog').showModal(); }
  } catch (error) { connection(false, 'Disconnected'); notice('Cannot reach server.', boot); }
}
render(); boot();
