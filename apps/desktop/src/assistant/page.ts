// The command center's Assistant page. Its keys are optional: OpenRouter for chat and Firecrawl
// for web search, each saved in the mesh through Credentials. The assistant's chats go above the
// keys, in `draw`.
import { el as h } from "../ui/dom.ts";
import { dbtn as button, secHead } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { views } from "../center/state.ts";
import { registerView } from "../center/views.ts";
import type { Credential } from "../settings/credentials.ts";
import { type KeyRow, KEYS_NOTE, keyRows, newKey } from "./assistant-state.ts";

/// The saved credentials (null if they could not be read) and what each row is doing.
const page = { list: undefined as Credential[] | null | undefined, busy: new Set<string>(), errors: new Map<string, string>() };

async function load() {
  page.list = await invoke<Credential[]>("list_credentials").catch(e => { console.warn("listing credentials:", e); return null; });
  draw();
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

function draw() {
  const el = views.get("assistant")?.el;
  if (!el) return;
  const keys = page.list === undefined ? [] : keyRows(page.list);
  const view = h("div", "cc-view",
    // The chats go here.
    h("div", "", secHead("Keys", h("small", "", KEYS_NOTE)),
      page.list === null
        ? h("div", "cc-panel cc-empty", h("span", "", "Could not read your credentials."))
        : h("div", "cc-panel", ...keys.map(keyLine))));
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
