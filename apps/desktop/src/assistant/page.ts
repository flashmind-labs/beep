// The command center's Assistant page. Its keys are optional: OpenRouter for chat and Firecrawl
// for web search, each saved in the mesh through Credentials. With an OpenRouter key the page
// lists the chats kept on this Mac above the keys and opens one as a conversation; without one it
// is only the keys. Firecrawl alone turns nothing on.
import { el as h } from "../ui/dom.ts";
import { dbtn as button, secHead } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { since } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { views } from "../center/state.ts";
import { registerView } from "../center/views.ts";
import { composeField, composeMessage } from "../chat/compose.ts";
import { copyCode } from "../agents/turnrows.ts";
import type { Credential } from "../settings/credentials.ts";
import type { ChatBody } from "../ui/turn.ts";
import { type KeyRow, KEYS_NOTE, keyRows, newKey } from "./assistant-state.ts";
import { type ChatSummary, type ModelList, type OpenChat, canChat, filterModels, localDate, modelLabel, webOn } from "./chats-state.ts";
import { endTurn, failTurn, savedTurn, startTurn, streamInto } from "./chat.ts";

/// The chat the page has open: its scrolling body stays while the page is redrawn, and while the
/// reply keeps coming with another page shown.
interface Open {
  id: string;
  title: string;
  model: string;
  body: ChatBody;
}

/// The saved credentials (null if they could not be read) and what each row is doing.
const page = {
  list: undefined as Credential[] | null | undefined,
  busy: new Set<string>(),
  errors: new Map<string, string>(),
  chats: undefined as ChatSummary[] | null | undefined,
  /// Why the last open or delete failed.
  error: "",
  open: null as Open | null,
  models: undefined as ModelList | null | undefined,
  modelError: "",
  picker: false,
  /// What is typed in the field, kept while the page redraws.
  draft: "",
};
/// Replies being written, by chat id, each settling when it ends.
const running = new Map<string, Promise<void>>();
/// The bodies of the chats opened, so a reply that is still coming draws into its own.
const bodies = new Map<string, ChatBody>();
let field: HTMLTextAreaElement | null = null;

async function load() {
  page.list = await invoke<Credential[]>("list_credentials").catch(e => { console.warn("listing credentials:", e); return null; });
  const chat = page.list != null && canChat(keyRows(page.list));
  if (!chat) {
    page.models = undefined;
    page.open = null;
  }
  draw();
  if (chat) await Promise.all([loadChats(), loadModels()]);
}

async function loadChats() {
  page.chats = await invoke<ChatSummary[]>("assistant_chats").catch(e => { console.warn("listing chats:", e); return null; });
  draw();
}

/// Reads OpenRouter's model list once; a failure shows in the picker, which tries again when opened.
async function loadModels() {
  if (page.models) return;
  try {
    page.models = await invoke<ModelList>("assistant_models");
    page.modelError = "";
    const open = page.open;
    if (open && !open.model && page.models.default) open.model = page.models.default;
  } catch (e) {
    page.models = null;
    page.modelError = String(e);
  }
  draw();
}

function chatBody(): ChatBody {
  const body = h("div", "dbody as-body") as ChatBody;
  body.stuck = true;
  body.onscroll = () => { body.stuck = body.scrollHeight - body.scrollTop - body.clientHeight < 40; };
  // Links in a reply open in the browser; a code block's button copies it.
  body.addEventListener("click", e => {
    const t = e.target as HTMLElement;
    const code = t.closest<HTMLElement>(".dcoderen.agent [data-copy]");
    if (code) return copyCode(code);
    const a = t.closest<HTMLAnchorElement>("a[href]");
    if (!a) return;
    e.preventDefault();
    if (/^https?:\/\//.test(a.href)) invoke("open_url", { url: a.href }).catch(err => console.warn("opening the link:", err));
  });
  return body;
}

const follow = (body: ChatBody) => () => { if (body.stuck !== false) body.scrollTop = body.scrollHeight; };

async function openChat(id: string) {
  page.error = "";
  let next: Open;
  try {
    const live = running.has(id) ? bodies.get(id) : undefined;
    const meta = page.chats?.find(c => c.id === id);
    if (live && meta) {
      next = { id, title: meta.title, model: meta.model, body: live };
    } else {
      const saved = await invoke<OpenChat>("assistant_open", { id });
      const body = chatBody();
      body.append(...saved.turns.map(savedTurn));
      bodies.set(id, body);
      next = { id, title: saved.title, model: saved.model, body };
    }
  } catch (e) {
    page.error = String(e);
    return draw();
  }
  page.open = next;
  page.picker = false;
  page.draft = "";
  draw();
  follow(next.body)();
}

function newChat() {
  const body = chatBody();
  const id = crypto.randomUUID();
  bodies.set(id, body);
  page.open = { id, title: "", model: page.models?.default ?? "", body };
  page.picker = false;
  page.draft = "";
  page.error = "";
  draw();
  if (!page.models) void loadModels();
}

function closeChat() {
  page.open = null;
  page.picker = false;
  draw();
  void loadChats();
}

async function removeChat(id: string) {
  page.error = "";
  try {
    if (running.has(id)) {
      await invoke("assistant_stop", { id });
      await running.get(id);
    }
    await invoke("assistant_delete", { id });
    bodies.delete(id);
  } catch (e) {
    page.error = String(e);
  }
  await loadChats();
}

async function send() {
  const open = page.open;
  if (!open || !field || running.has(open.id)) return;
  const text = composeMessage(field.value).trim();
  if (!text) return;
  if (!open.model) {
    await loadModels();
    open.model = page.models?.default ?? "";
  }
  if (!open.model) {
    page.error = page.modelError || "Pick a model first.";
    return draw();
  }
  page.error = "";
  field.value = "";
  page.draft = "";
  const { id, body } = open;
  const turn = startTurn(text);
  body.append(turn);
  open.title ||= text.split("\n")[0].slice(0, 60);
  body.stuck = true;
  const scroll = follow(body);
  scroll();
  const message = { id, model: open.model, message: text, today: localDate() };
  const run = (async () => {
    try {
      await invoke("assistant_send", { prompt: message, onEvent: streamInto(turn, scroll, () => {}) });
    } catch (e) {
      failTurn(turn, String(e));
    } finally {
      if (!turn.done) endTurn(turn);
      running.delete(id);
      scroll();
      await loadChats();
    }
  })();
  running.set(id, run);
  draw();
}

function stop() {
  const id = page.open?.id;
  if (id) invoke("assistant_stop", { id }).catch(e => console.warn("stopping the reply:", e));
}

// ---------- Keys ----------

function savedRow(row: KeyRow) {
  const saved = row.saved!;
  const remove = button("Remove", "quiet small", async () => {
    remove.disabled = true;
    try {
      await invoke("remove_credential", { id: saved.id });
      page.errors.delete(row.provider.slug);
    } catch (e) {
      page.errors.set(row.provider.slug, String(e));
    }
    await load();
  });
  const shown = saved.unreadable ? h("span", "derr", "Enter it again") : h("code", "", `…${saved.last4}`);
  return h("span", "r", shown, remove);
}

function emptyRow(row: KeyRow) {
  const slug = row.provider.slug;
  const input = h("input", "dsecret");
  input.type = "password";
  input.placeholder = "Paste the key";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", `${row.provider.name} key`);
  const save = button(page.busy.has(slug) ? "Checking…" : "Check and save", "pri small", async () => {
    if (!input.value.trim()) return;
    page.busy.add(slug);
    page.errors.delete(slug);
    draw();
    try {
      await invoke("save_credential", { credential: newKey(row.provider, input.value) });
    } catch (e) {
      page.errors.set(slug, String(e));
    }
    page.busy.delete(slug);
    await load();
  });
  save.disabled = page.busy.has(slug);
  input.addEventListener("keydown", e => { if (e.key === "Enter") save.click(); });
  const get = button("Get a key", "quiet small", () => { invoke("open_url", { url: row.provider.url }).catch(e => console.warn("opening the link:", e)); });
  return h("span", "r", input, get, save);
}

function keyLine(row: KeyRow) {
  const err = page.errors.get(row.provider.slug);
  const line = h("div", "cc-row cc-line bare", h("span", "t", h("b", "", row.provider.name), h("span", "", row.provider.use)), row.saved ? savedRow(row) : emptyRow(row));
  return err ? h("div", "", line, h("div", "derr", err)) : line;
}

function keysSection(keys: KeyRow[]) {
  return h("div", "", secHead("Keys", h("small", "", KEYS_NOTE)),
    page.list === null
      ? h("div", "cc-panel cc-empty", h("span", "", "Could not read your credentials."))
      : h("div", "cc-panel", ...keys.map(keyLine)));
}

// ---------- Chats ----------

function chatRow(c: ChatSummary) {
  const open = h("button", "t", h("b", "", c.title || "New chat"), h("span", "", `${c.model} · ${since(c.updated_ms)}`));
  open.type = "button";
  open.title = c.title;
  open.addEventListener("click", () => void openChat(c.id));
  const del = button("Delete", "quiet small", () => { del.disabled = true; void removeChat(c.id); });
  return h("div", "cc-row cc-line bare", open, h("span", "r", del));
}

function chatsSection() {
  const add = button("New chat", "small", newChat);
  add.style.marginLeft = "auto";
  const body = page.chats === undefined ? null
    : page.chats === null ? h("div", "cc-panel cc-empty", h("span", "", "Could not read your chats."))
    : page.chats.length === 0 ? h("div", "cc-panel cc-empty", h("b", "", "No chats yet"), h("span", "", "Ask about your machines, projects and coding agents."))
    : h("div", "cc-panel", ...page.chats.map(chatRow));
  return h("div", "", secHead("Chats", h("small", "", "Kept on this Mac only."), add), page.error ? h("div", "derr", page.error) : null, body);
}

/// An element holding the SVG `html`.
function icon(html: string) {
  const span = h("span", "ck");
  span.innerHTML = html;
  return span;
}

function modelPicker(open: Open) {
  const search = h("input");
  search.placeholder = page.models ? `Search ${page.models.models.length} models` : "Search models";
  search.autocomplete = "off";
  search.spellcheck = false;
  search.setAttribute("aria-label", "Search models");
  const list = h("div", "as-models");
  list.setAttribute("role", "listbox");
  const fill = () => {
    if (page.models === undefined) return list.replaceChildren(h("div", "cc-empty", "Loading models…"));
    if (page.models === null) {
      const again = button("Try again", "small", () => { page.models = undefined; void loadModels(); });
      return list.replaceChildren(h("div", "cc-empty", h("span", "derr", page.modelError), again));
    }
    const rows = filterModels(page.models.models, search.value).map(m => {
      const row = h("button", "cc-row as-model", h("span", "t", h("b", "", m.name), h("span", "", m.id)), h("span", "r", m.id === open.model ? icon(ICON.check) : null, m.tools ? null : h("span", "dtag", "no tools")));
      row.type = "button";
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", String(m.id === open.model));
      row.addEventListener("click", () => { open.model = m.id; page.picker = false; draw(); });
      return row;
    });
    list.replaceChildren(...(rows.length ? rows : [h("div", "cc-empty", "No model matches.")]));
  };
  search.addEventListener("input", fill);
  search.addEventListener("keydown", e => { if (e.key === "Escape") { e.stopPropagation(); page.picker = false; draw(); } });
  fill();
  queueMicrotask(() => search.focus());
  const box = h("div", "as-pick", h("div", "dinput small", search), list);
  return box;
}

function chatView(rows: KeyRow[]) {
  const open = page.open!;
  const busy = running.has(open.id);
  const pill = button(`${modelLabel(page.models?.models ?? null, open.model)} `, "small as-pill", () => {
    page.picker = !page.picker;
    if (page.picker && !page.models) void loadModels();
    draw();
  });
  pill.setAttribute("aria-haspopup", "listbox");
  pill.setAttribute("aria-expanded", String(page.picker));
  pill.insertAdjacentHTML("beforeend", ICON.chev);
  const head = h("div", "as-head", button("Chats", "quiet small", closeChat),
    h("span", "t", h("b", "", open.title || "New chat"), h("small", "", webOn(rows) ? "Web search on" : "Web search off")), pill);
  const ta = h("textarea");
  ta.rows = 1;
  ta.placeholder = "Message";
  ta.autocomplete = "off";
  ta.spellcheck = false;
  ta.setAttribute("aria-label", "Message");
  ta.value = page.draft;
  ta.addEventListener("input", () => { page.draft = ta.value; });
  ta.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); void send(); }
  });
  const act = busy ? button("Stop", "small", stop) : button("Send", "pri small", () => void send());
  const box = h("div", "dinput small", ta, act);
  field = ta;
  const panel = h("div", "cc-panel as-chat", head, open.body, page.error ? h("div", "derr as-err", page.error) : null, box, page.picker ? modelPicker(open) : null);
  // The field is a composer once it is in its box, so long pastes become pills.
  composeField(ta, { grow: 156 });
  if (!page.picker) requestAnimationFrame(() => ta.focus());
  requestAnimationFrame(follow(open.body));
  return panel;
}

function draw() {
  const el = views.get("assistant")?.el;
  if (!el) return;
  const rows = page.list === undefined ? [] : keyRows(page.list);
  const chat = page.list != null && canChat(rows);
  const view = h("div", "cc-view",
    ...(chat && page.open ? [chatView(rows)] : [chat ? chatsSection() : null, keysSection(rows)]));
  const old = el.querySelector(":scope > .cc-view");
  if (old) old.replaceWith(view); else el.append(view);
}

registerView("assistant", {
  title: () => ({ title: "Assistant", sub: "" }),
  mount() {
    draw();
    load();
  },
});
