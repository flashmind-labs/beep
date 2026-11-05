// The command center's Assistant page: the two keys the local assistant takes, both optional,
// and the MCP servers whose tools it may use. OpenRouter is for chat and Firecrawl for web search,
// each saved in the mesh through Credentials. With an OpenRouter key and no server the chats are
// the Chats group in the sidebar and the chat pages, the same as with a server; this page is only
// where the keys and the servers live. The servers are listed in a file of this Mac (Rust,
// `assistant/mcp.rs`); an env value or a token is sent once and never comes back.
import { el as h } from "../ui/dom.ts";
import { dbtn as button, emptyPanel, secHead } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { views } from "../center/state.ts";
import { registerView } from "../center/views.ts";
import { reloadGroup } from "../center/sidebar.ts";
import { loadModels } from "../settings/model.ts";
import type { Credential } from "../settings/credentials.ts";
import { blankMcpForm, type KeyRow, KEYS_NOTE, keyRows, MCP_NOTE, type McpForm, type McpRow, mcpDetail, mcpFormReady, mcpStatus, newKey, newMcpServer } from "./assistant-state.ts";
import { refreshLocal } from "./local.ts";
import { S } from "../bar/core.ts";

/// The saved credentials (null if they could not be read) and what each row is doing.
const page = { list: undefined as Credential[] | null | undefined, busy: new Set<string>(), errors: new Map<string, string>() };

/// The MCP servers (null if the list could not be read), the add form while it is open, and what
/// each row or the form is doing.
const mcp = { rows: undefined as McpRow[] | null | undefined, form: null as McpForm | null, saving: false, busy: new Set<string>(), errors: new Map<string, string>(), formError: "" };

async function loadMcp() {
  mcp.rows = await invoke<McpRow[]>("list_mcp_servers").catch(e => { console.warn("listing MCP servers:", e); return null; });
  draw();
}

async function load() {
  page.list = await invoke<Credential[]>("list_credentials").catch(e => { console.warn("listing credentials:", e); return null; });
  draw();
  // A key saved or removed turns the chats on or off: the sidebar and the model chip follow.
  if (await refreshLocal()) {
    for (const name of ["pinned", "chats"]) void reloadGroup(name);
    if (S.local) void loadModels();
  }
}

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

const MONO = "var(--mono)";

function mcpLine(row: McpRow) {
  const name = row.name;
  const status = mcpStatus(row);
  const err = mcp.errors.get(name) ?? (row.status.state === "failed" ? row.status.error : "");
  // One command at a time per row; the list is read again once it ends.
  const act = (label: string, command: string) => {
    const b = button(label, "quiet small", async () => {
      mcp.busy.add(name);
      mcp.errors.delete(name);
      draw();
      try {
        await invoke(command, { name });
      } catch (e) {
        mcp.errors.set(name, String(e));
      }
      mcp.busy.delete(name);
      await loadMcp();
    });
    b.disabled = mcp.busy.has(name);
    return b;
  };
  const detail = h("span", "", mcpDetail(row));
  detail.style.fontFamily = MONO;
  const state = h("span", status.failed ? "derr" : "", mcp.busy.has(name) ? "Working…" : status.text);
  const line = h("div", "cc-row cc-line bare", h("span", "t", h("b", "", name), detail), h("span", "r", state, act("Check", "check_mcp_server"), act("Remove", "remove_mcp_server")));
  return err ? h("div", "", line, h("div", "derr", err)) : line;
}

function field(label: string, value: string, set: (v: string) => void, opts: { placeholder: string; secret?: boolean; lines?: number }) {
  const input = opts.lines ? h("textarea", "dsecret") : h("input", "dsecret");
  if (input instanceof HTMLInputElement) input.type = opts.secret ? "password" : "text";
  if (input instanceof HTMLTextAreaElement) input.rows = opts.lines!;
  input.value = value;
  input.placeholder = opts.placeholder;
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", label);
  input.addEventListener("input", () => set(input.value));
  const box = h("label", "", label, input);
  box.style.cssText = "display:grid;gap:4px;font-size:12.5px;color:var(--muted)";
  return { box, input };
}

function mcpForm(form: McpForm) {
  const tab = (label: string, kind: McpForm["kind"]) => button(label, `small ${form.kind === kind ? "pri" : "quiet"}`, () => { form.kind = kind; draw(); });
  const save = button(mcp.saving ? "Adding…" : "Add", "pri small", async () => {
    if (!mcpFormReady(form) || mcp.saving) return;
    mcp.saving = true;
    mcp.formError = "";
    draw();
    try {
      await invoke("add_mcp_server", { server: newMcpServer(form) });
      mcp.form = null;
    } catch (e) {
      mcp.formError = String(e);
    }
    mcp.saving = false;
    await loadMcp();
  });
  save.disabled = mcp.saving;
  const enter = (e: KeyboardEvent) => { if (e.key === "Enter") save.click(); };
  const name = field("Name", form.name, v => { form.name = v; }, { placeholder: form.kind === "command" ? "files" : "notes" });
  const kind = form.kind === "command"
    ? [field("Command", form.command, v => { form.command = v; }, { placeholder: "npx -y @example/files-mcp /srv/docs" }),
       field("Environment (optional)", form.env, v => { form.env = v; }, { placeholder: "API_KEY=...", lines: 2 })]
    : [field("URL", form.url, v => { form.url = v; }, { placeholder: "https://example.test/mcp" }),
       field("Token (optional)", form.token, v => { form.token = v; }, { placeholder: "Bearer token", secret: true })];
  for (const f of [name, ...kind]) if (f.input instanceof HTMLInputElement) f.input.addEventListener("keydown", enter);
  const cancel = button("Cancel", "quiet small", () => { mcp.form = null; mcp.formError = ""; draw(); });
  const row = (...kids: Node[]) => {
    const r = h("span", "", ...kids);
    r.style.cssText = "display:flex;gap:8px";
    return r;
  };
  const box = h("div", "", row(tab("Command", "command"), tab("URL", "url")), name.box, ...kind.map(f => f.box), row(save, cancel), mcp.formError ? h("div", "derr", mcp.formError) : null);
  box.style.cssText = "display:grid;gap:10px;padding:12px 14px";
  return box;
}

function mcpSection() {
  const add = mcp.form ? null : button("Add a server", "small", () => { mcp.form = blankMcpForm(); draw(); });
  add?.style.setProperty("white-space", "nowrap");
  const head = secHead("MCP servers", h("small", "", MCP_NOTE), add);
  head.querySelector("h4")!.style.whiteSpace = "nowrap";
  let body;
  if (mcp.form) body = h("div", "cc-panel", mcpForm(mcp.form));
  else if (mcp.rows === null) body = emptyPanel(h("span", "", "Could not read your MCP servers."));
  else if (!mcp.rows?.length) body = emptyPanel(h("span", "", mcp.rows ? "No servers yet" : ""));
  else body = h("div", "cc-panel", ...mcp.rows.map(mcpLine));
  return h("div", "", head, body);
}

function draw() {
  const el = views.get("assistant")?.el;
  if (!el) return;
  const keys = page.list === undefined ? [] : keyRows(page.list);
  const view = h("div", "cc-view",
    h("div", "", secHead("Keys", h("small", "", KEYS_NOTE)),
      page.list === null
        ? h("div", "cc-panel cc-empty", h("span", "", "Could not read your credentials."))
        : h("div", "cc-panel", ...keys.map(keyLine))),
    mcpSection());
  const old = el.querySelector(":scope > .cc-view");
  if (old) old.replaceWith(view); else el.append(view);
}

registerView("assistant", {
  title: () => ({ title: "Assistant", sub: "" }),
  mount() {
    draw();
    load();
    loadMcp();
  },
});
