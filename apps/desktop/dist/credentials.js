// Settings, Credentials: git hosts (GitHub, one row per Forgejo server)
// then one API key per provider. Secrets are typed here, sent once and
// never shown again: the server gives back only their last 4 characters.

/// `git.example.com` from `https://git.example.com/`.
export function hostOf(url) {
  return String(url || "").replace(/^[a-z]+:\/\//i, "").replace(/\/+$/, "");
}

/// The names of the machines that report `tool`, or null when the machines
/// are not known (only admins may list them).
export function machinesWith(machines, tool) {
  if (!Array.isArray(machines)) return null;
  return machines.filter(m => m.env?.tools?.some(t => t.name === tool)).map(m => m.name);
}

/// "gh on build-box, mac-box", or "" when the machines are not known.
export function toolLine(cli, machines) {
  const on = machinesWith(machines, cli);
  if (on === null) return "";
  return on.length ? `${cli} on ${on.join(", ")}` : `${cli} on no machine yet`;
}

/// What Save sends for the row being edited, or an error to show.
export function draft(edit, url, secret, toMachines) {
  secret = String(secret || "").trim();
  if (!edit) return { error: "Nothing to save." };
  if (!secret) return { error: "Paste the token first." };
  if (edit.kind === "forgejo") {
    const u = String(url || edit.url || "").trim();
    if (!/^https?:\/\/[^\s/]+/i.test(u)) return { error: "The server URL starts with https://, such as https://git.example.com." };
    return { credential: { kind: "forgejo", url: u, secret, to_machines: true } };
  }
  if (edit.kind === "github") return { credential: { kind: "github", secret, to_machines: true } };
  return { credential: { kind: "api_key", provider: edit.provider, secret, to_machines: !!toMachines } };
}

const sm = 'style="font-size:11.5px;padding:2px 8px;min-height:24px"';

/// The inline form for adding or replacing one credential.
function form(state, esc, { title, note, env, url }) {
  const e = state.edit;
  const urlField = e.kind === "forgejo" && !e.id
    ? `<input id="credUrl" class="url" type="url" placeholder="https://git.example.com" autocomplete="off" spellcheck="false" value="${esc(e.url ?? "")}" aria-label="Server URL">`
    : "";
  const check = e.kind === "api_key" && env
    ? `<label style="display:flex;gap:8px;align-items:center;font-size:12px;color:var(--muted)"><input type="checkbox" id="credMachines"${e.toMachines !== false ? " checked" : ""}> Give it to my machines as ${esc(env)}</label>`
    : "";
  const label = e.kind === "api_key" && e.provider !== "openrouter" ? "Save" : "Check and save";
  return `<div class="dcred-form" style="padding:10px 0;border-bottom:1px solid var(--border);display:grid;gap:8px">
    <div style="display:flex;align-items:center;gap:8px;font-size:13px"><b style="font-weight:600">${esc(title)}</b>${url ? `<span style="color:var(--muted);font-size:12px">${esc(url)}</span>` : ""}</div>
    <div class="dfield${state.error ? " err" : ""}" style="gap:8px">${urlField}
      <div style="display:flex;gap:8px"><input id="credSecret" type="password" style="flex:1;font-family:var(--mono);font-size:12.5px" placeholder="Paste the token" autocomplete="off" spellcheck="false" aria-label="Token">
      <button class="dbtn pri" id="credSave"${state.busy ? " disabled" : ""}>${state.busy ? "Checking…" : label}</button><button class="dbtn quiet" id="credCancel">Cancel</button></div></div>
    ${check}${note ? `<span style="font-size:11.5px;color:var(--muted)">${esc(note)}</span>` : ""}
    <div class="derr" id="credErr">${esc(state.error || "")}</div></div>`;
}

/// The last 4 of a saved secret, or a prompt to enter it again when the
/// server can no longer open it.
function shown(esc, cred, what) {
  return cred.unreadable
    ? `<span style="color:var(--err)">Re-enter this ${what}</span>`
    : `<code style="font-size:11px">…${esc(cred.last4)}</code>`;
}

/// One saved git host: name, login, last 4 and where its CLI is.
function hostRow(esc, { name, cred, cli, machines, editKey }) {
  const line = [cred ? shown(esc, cred, "token") : "", esc(toolLine(cli, machines))].filter(Boolean).join(" · ");
  const account = cred?.unreadable ? "" : cred?.account ? `@${cred.account}` : cred ? "not checked: the server could not reach it" : "";
  const acts = cred
    ? `<button class="dbtn" ${sm} data-cred-edit="${esc(editKey)}">Replace</button><button class="dbtn quiet" ${sm} data-cred-remove="${esc(cred.id)}" aria-label="Remove ${esc(name)}">Remove</button>`
    : `<button class="dbtn" ${sm} data-cred-edit="${esc(editKey)}">Add token</button>`;
  return `<div style="padding:9px 0;border-bottom:1px solid var(--border);display:grid;gap:3px;font-size:13px">
    <div style="display:flex;align-items:center;gap:8px;min-width:0"><span class="ddot${cred ? "" : " off"}"></span><b style="font-weight:600">${esc(name)}</b>${account ? `<span style="color:var(--muted);font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${esc(account)}</span>` : ""}<span style="margin-left:auto;display:flex;gap:6px;flex:none">${acts}</span></div>
    ${line ? `<span style="font-size:11.5px;color:var(--muted);padding-left:15px">${line}</span>` : ""}</div>`;
}

/// One provider's API key row.
function keyRow(esc, ago, p, cred) {
  if (!cred) return `<div style="display:flex;align-items:center;gap:8px;padding:9px 0;border-bottom:1px solid var(--border);font-size:13px">
    <span class="ddot off"></span><b style="font-weight:600">${esc(p.name)}</b><code style="font-size:11px;color:var(--muted)">${esc(p.env_var)}</code>
    <span style="margin-left:auto"><button class="dbtn" ${sm} data-cred-edit="api:${esc(p.slug)}">Add key</button></span></div>`;
  const use = cred.unreadable ? "not given to your machines" : cred.to_machines ? `your machines as ${esc(p.env_var)}` : "not given to your machines";
  return `<div style="display:flex;align-items:center;gap:8px;padding:9px 0;border-bottom:1px solid var(--border);font-size:13px">
    <span class="ddot"></span><span style="display:grid;gap:2px;min-width:0"><b style="font-weight:600">${esc(p.name)}</b><span style="font-size:11.5px;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${shown(esc, cred, "key")} · ${use} · added ${esc(ago(cred.created_at))}</span></span>
    <span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn" ${sm} data-cred-edit="api:${esc(p.slug)}">Replace</button><button class="dbtn quiet" ${sm} data-cred-remove="${esc(cred.id)}" aria-label="Remove the ${esc(p.name)} key">Remove</button></span></div>`;
}

const head = (text, top) => `<div style="font-size:11px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;color:var(--dim);margin:${top}px 0 2px">${text}</div>`;

/// The whole section. `state`: { list, providers, edit, busy, error };
/// `machines`: the machines list, or null when it is not known.
export function credentialsHtml(state, machines, esc, ago) {
  const list = state.list ?? [];
  const e = state.edit;
  const github = list.find(c => c.kind === "github");
  const forgejo = list.filter(c => c.kind === "forgejo");
  let html = head("Git hosts", 4);
  html += e?.kind === "github"
    ? form(state, esc, { title: "GitHub", note: "Make one on GitHub: Settings, Developer settings, Personal access tokens, with repo access." })
    : hostRow(esc, { name: "GitHub", cred: github, cli: "gh", machines, editKey: "github" });
  for (const c of forgejo) {
    html += e?.kind === "forgejo" && e.id === c.id
      ? form(state, esc, { title: hostOf(c.url), url: "Replace the token" })
      : hostRow(esc, { name: hostOf(c.url), cred: c, cli: "teacli", machines, editKey: `forgejo:${c.id}` });
  }
  html += e?.kind === "forgejo" && !e.id
    ? form(state, esc, { title: "New Forgejo server", url: "one token per server, teacli on every machine", note: "Make one in Forgejo: Settings, Applications, Access tokens, with repository and issue access." })
    : `<button class="dbtn" style="font-size:11.5px;padding:3px 9px;margin-top:8px;justify-self:start" data-cred-edit="forgejo">+ Add a Forgejo server</button>`;
  html += head("API keys", 14);
  html += `<div style="font-size:12.5px;color:var(--muted);margin:4px 0 8px;max-width:60ch">Your own keys, one per provider. Beep stores them encrypted, shows only the last 4 characters, and, when you choose, gives each to commands and terminals on your machines as its env var. Agents do not get API keys.</div>`;
  for (const p of state.providers ?? []) {
    const cred = list.find(c => c.kind === "api_key" && c.provider === p.slug);
    html += e?.kind === "api_key" && e.provider === p.slug
      ? form(state, esc, { title: p.name, env: p.env_var, url: p.env_var })
      : keyRow(esc, ago, p, cred);
  }
  return `<div style="display:grid">${html}</div>`;
}

/// The edit a `data-cred-edit` value names, with what the form starts with.
export function editFor(key, list) {
  if (key === "github") return { kind: "github" };
  if (key === "forgejo") return { kind: "forgejo" };
  if (key.startsWith("forgejo:")) {
    const id = key.slice("forgejo:".length);
    const c = (list ?? []).find(x => x.id === id);
    return c ? { kind: "forgejo", id, url: c.url } : { kind: "forgejo" };
  }
  if (key.startsWith("api:")) {
    const provider = key.slice(4);
    const c = (list ?? []).find(x => x.kind === "api_key" && x.provider === provider);
    return { kind: "api_key", provider, toMachines: c ? c.to_machines : true };
  }
  return null;
}
