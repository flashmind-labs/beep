// Settings, Agent accounts: how the coding agents sign in on your machines.
// Claude Code signs in once here and Beep hands its token to the machines;
// Codex signs in on each machine, or uses an API key Beep hands out.

const sm = 'style="font-size:11.5px;padding:2px 8px"';
const box = inner => `<div style="border:1px solid var(--border);border-radius:10px;padding:12px 14px;margin-top:12px">${inner}</div>`;
const row = (name, off, act) => `<div style="display:flex;align-items:center;gap:8px;padding:6px 0;font-size:12.5px"><span class="ddot${off ? " off" : ""}"></span><span>${name}</span><span style="margin-left:auto;color:var(--muted);display:flex;gap:6px;align-items:center">${act}</span></div>`;

/// "2 sessions now", "idle", or "offline · gets it when back": how a
/// machine with Claude Code stands with the token.
export function usedOn(machine) {
  if (!machine.online) return "offline · gets it when back";
  const n = machine.claude_sessions || 0;
  if (!n) return "idle";
  return n === 1 ? "1 session now" : `${n} sessions now`;
}

/// How a machine's own Claude Code sign-in stands: "signed in as you@example.com",
/// "not signed in", or "not checked" when it never said.
export function ownLogin(login) {
  if (!login) return "not checked";
  if (!login.signed_in) return "not signed in";
  return login.account ? `signed in as ${login.account}` : "signed in";
}

/// "14:32 left" until `expiresAt` (ms), or "expired".
export function timeLeft(expiresAt, now) {
  const s = Math.floor((expiresAt - now) / 1000);
  if (!(s > 0)) return "expired";
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")} left`;
}

/// What Save sends for the token being typed, or an error to show.
export function tokenDraft(agent, secret) {
  secret = String(secret || "").trim();
  if (agent !== "claude" && agent !== "codex") return { error: "Nothing to save." };
  if (!secret) return { error: agent === "claude" ? "Paste the token first." : "Paste the key first." };
  if (/\s/.test(secret)) return { error: "That does not look like a token: it has spaces in it." };
  if (agent === "claude" && secret.startsWith("sk-ant-api")) return { error: "That is an Anthropic API key. Paste the token claude setup-token prints." };
  return { agent, secret };
}

/// The machines with `tool` ("claude" or "codex"), by name.
export function machinesWith(machines, tool) {
  return (machines ?? []).filter(m => m[tool]);
}

/// The paste form for `agent`.
function form(state, esc, agent) {
  const claude = agent === "claude";
  const note = claude
    ? `Run <code style="font-size:11px">claude setup-token</code> in Terminal and paste the token it prints.`
    : `An OpenAI API key. Codex uses it on every machine instead of signing in.`;
  return `<div style="display:grid;gap:8px;margin-top:10px">
    <div style="display:flex;gap:8px"><input id="agSecret" type="password" autocomplete="off" spellcheck="false" placeholder="${claude ? "sk-ant-oat01-…" : "sk-…"}" aria-label="${claude ? "Claude Code token" : "Codex API key"}" style="flex:1;font-family:var(--mono,monospace);font-size:12px">
    <button class="dbtn pri" id="agSave" data-ag-agent="${agent}"${state.busy ? " disabled" : ""}>${state.busy === agent ? "Saving…" : "Save"}</button><button class="dbtn quiet" id="agCancel">Cancel</button></div>
    <span style="font-size:11.5px;color:var(--muted)">${note}</span>
    <span class="derr" id="agErr">${esc(state.error || "")}</span></div>`;
}

function claudeHtml(state, esc, ago) {
  const token = state.data.claude;
  const on = machinesWith(state.data.machines, "claude");
  const flow = state.claudeFlow;
  const head = sub => `<div style="display:flex;align-items:center;gap:10px"><span class="cc-ag claude">CC</span><span style="display:grid;gap:2px;min-width:0"><b style="font-size:13.5px">Claude Code</b>${sub ? `<span style="font-size:11.5px;color:var(--muted)">${sub}</span>` : ""}</span>`;
  const err = state.claudeError ? `<div class="derr" style="margin-top:6px">${esc(state.claudeError)}</div>` : "";
  if (flow) {
    return box(`${head("")}<span style="margin-left:auto"><button class="dbtn quiet" ${sm} data-ag-claude-cancel>Cancel</button></span></div>
      <div style="font-size:12px;color:var(--muted);margin-top:8px;display:flex;gap:8px;align-items:center"><span class="dspin"></span>Approve in your browser. Beep saves the token once you do.</div>`);
  }
  if (!token || token.unreadable) {
    const why = token?.unreadable ? "Beep can no longer read the saved token. Sign in again." : "Sign in for all to give every machine one token. Beep keeps it encrypted and never writes it to their disks.";
    const edit = state.edit === "claude" ? form(state, esc, "claude") : "";
    const rows = on.map(m => row(esc(m.name), !m.online, esc(ownLogin(state.data.claude_logins?.get(m.id))))).join("");
    return box(`${head(on.length ? "signs in on each machine" : "")}<span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn${on.length ? "" : " pri"}" data-ag-claude-sign>${on.length ? "Sign in for all" : "Sign in"}</button>${edit ? "" : `<button class="dbtn quiet" data-ag-edit="claude">Paste a token</button>`}</span></div>
      ${rows ? `<div style="margin-top:6px">${rows}</div>` : ""}<div style="font-size:12px;color:var(--muted);margin-top:6px">${why}</div>${edit}${err}`);
  }
  const sub = `signed in ${esc(ago(token.created_at))} · <code style="font-size:11px">…${esc(token.last4)}</code>`;
  const edit = state.edit === "claude" ? form(state, esc, "claude") : "";
  const used = on.length
    ? on.map(m => row(esc(m.name), !m.online, esc(usedOn(m)))).join("")
    : `<div style="font-size:12px;color:var(--muted);padding:6px 0">No machine has Claude Code yet.</div>`;
  return box(`${head(sub)}<span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn" data-ag-claude-sign>Replace</button><button class="dbtn warn" data-ag-revoke="claude"${state.busy === "revoke:claude" ? " disabled" : ""}>Revoke</button></span></div>
    ${edit}${err}<div style="font-size:12px;color:var(--muted);margin:8px 0 4px">Used on</div>${used}
    ${edit ? "" : `<button class="dbtn quiet" ${sm} data-ag-edit="claude">Paste a token instead</button>`}`);
}

/// One machine's Codex row: its sign-in, the code it waits on, or why
/// it failed.
function codexRow(state, esc, m, now) {
  const flow = state.codex?.[m.id];
  if (flow?.code) {
    return `<div style="padding:8px 10px;margin:4px 0;border-radius:8px;background:var(--elev);border:1px solid var(--border);font-size:12.5px;display:grid;gap:6px">
      <span>${esc(m.name)}: open <b>${esc(flow.url.replace(/^https:\/\//, ""))}</b> and enter</span>
      <span style="font-family:var(--mono,monospace);font-size:18px;letter-spacing:.12em;user-select:all">${esc(flow.code)}</span>
      <span style="display:flex;gap:6px;align-items:center;color:var(--muted)"><span class="dspin"></span>Waiting for you to approve · ${esc(timeLeft(flow.expires_at, now))}<span style="margin-left:auto;display:flex;gap:6px"><button class="dbtn" ${sm} data-ag-copy="${esc(flow.code)}">${flow.copied ? "Copied" : "Copy code"}</button><button class="dbtn quiet" ${sm} data-ag-open="${esc(flow.url)}">Open page</button><button class="dbtn quiet" ${sm} data-ag-codex-cancel="${esc(m.id)}">Cancel</button></span></span></div>`;
  }
  const err = flow?.error ? `<div class="derr" style="padding-left:15px">${esc(flow.error)}</div>` : "";
  if (!m.online) return row(esc(m.name), true, "offline") + err;
  if (flow?.starting) return row(esc(m.name), false, `<span class="dspin"></span>starting sign-in`);
  if (m.codex_signed_in) return row(esc(m.name), false, "signed in") + err;
  const label = m.codex_signed_in === false ? "" : `<span>not checked</span>`;
  return row(esc(m.name), false, `${label}<button class="dbtn" ${sm} data-ag-codex-sign="${esc(m.id)}">Sign in</button>`) + err;
}

function codexHtml(state, esc, ago, now) {
  const key = state.data.codex_key;
  const on = machinesWith(state.data.machines, "codex");
  const usable = key && !key.unreadable;
  const sub = usable
    ? `API key <code style="font-size:11px">…${esc(key.last4)}</code> on every machine · added ${esc(ago(key.created_at))}`
    : key ? "Beep can no longer read the saved API key" : "signs in on each machine";
  const acts = key
    ? `<button class="dbtn" ${sm} data-ag-edit="codex">Replace key</button><button class="dbtn quiet" ${sm} data-ag-revoke="codex"${state.busy === "revoke:codex" ? " disabled" : ""}>Remove key</button>`
    : `<button class="dbtn" data-ag-edit="codex">Use an API key</button>`;
  const edit = state.edit === "codex" ? form(state, esc, "codex") : "";
  const rows = on.length
    ? on.map(m => codexRow(state, esc, m, now)).join("")
    : `<div style="font-size:12px;color:var(--muted);padding:6px 0">No machine has Codex yet.</div>`;
  return box(`<div style="display:flex;align-items:center;gap:10px"><span class="cc-ag codex">CX</span><span style="display:grid;gap:2px;min-width:0"><b style="font-size:13.5px">Codex</b><span style="font-size:11.5px;color:var(--muted)">${sub}</span></span><span style="margin-left:auto;display:flex;gap:6px;flex:none">${edit ? "" : acts}</span></div>
    ${edit}<div style="margin-top:6px">${rows}</div>`);
}

/// The whole section. `state`: { data, edit, busy, error, claudeFlow,
/// claudeError, codex }. `data` is GetAgentAccounts' answer (undefined
/// until it came, null when it failed, false from a server too old);
/// `edit` the agent whose token is being typed; `codex` the sign-ins
/// under way by machine id: { starting } then { task, url, code,
/// expires_at, copied }, or { error }.
export function agentAccountsHtml(state, esc, ago, now = Date.now()) {
  if (state.data === null) return `<div class="dmuted">Could not load your agent accounts.</div>`;
  if (state.data === false) return `<div class="dmuted">The server needs an update before it keeps agent accounts.</div>`;
  return `<div style="font-size:12.5px;color:var(--muted);margin-top:4px">How your coding agents sign in on your machines.</div>${claudeHtml(state, esc, ago)}${codexHtml(state, esc, ago, now)}`;
}
