import { copy, when } from "./chats.ts";
import { Channel, esc, invoke, S } from "./core.ts";
import { fillSection, SET, setAccountHtml, setRoot, setShown, setSkeleton } from "./settings.ts";

/// Settings, Credentials (`credentials.js` draws it). `list` is undefined
/// until the server answers and null when it failed; `edit` is the row
/// whose token is being typed.
/// `cli` is what gh and teacli on this Mac are signed in to, `cliBusy` the
/// sign-in being saved and `cliError` why one was refused. `ghFlow` is gh's
/// browser sign-in under way: { code, url, copied }.
/// `github` is the GitHub accounts, undefined when the server could not
/// list them; `ghAdd` whether "Add another GitHub account" is open.
export const CRED = { mod: null, list: undefined, github: undefined, providers: [], edit: null, busy: false, error: "", cli: undefined, cliBusy: "", cliError: null, ghFlow: null, ghAdd: false };
export const credModule = () => import("../credentials.ts").then(m => (CRED.mod = m));

export function setCredsHtml() {
  if (CRED.list === undefined || !CRED.mod) return setSkeleton(2);
  if (CRED.list === null) return `<div class="dmuted">Could not load your credentials.</div>`;
  // Which machines have gh and teacli, when the machines are known.
  const machines = Array.isArray(SET.machines) ? SET.machines : null;
  return CRED.mod.credentialsHtml(CRED, machines, esc, when);
}

/// Asks gh and teacli on this Mac what they are signed in to, then redraws.
export async function loadCliLogins() {
  try {
    CRED.cli = await invoke("cli_logins");
  } catch (e) {
    console.warn(e);
    CRED.cli = null;
  }
  // Not while a token is typed: the redraw would drop it.
  if (setShown() && CRED.list && !setRoot().querySelector("#credSecret")?.value) fillSection("creds", setCredsHtml());
}

/// Lists the credentials again and redraws their section.
export async function loadCreds() {
  loadCliLogins();
  try {
    // A server from before several GitHub accounts does not list them:
    // the credentials list then has the one GitHub token.
    const accounts = invoke("list_github_accounts").catch(e => { console.warn(e); return undefined; });
    const [, list, providers, github] = await Promise.all([credModule(), invoke("list_credentials"), invoke("credential_providers"), accounts]);
    CRED.list = list;
    CRED.github = github;
    CRED.providers = providers;
  } catch (e) {
    console.warn(e);
    CRED.list ??= null;
  }
  if (setShown()) {
    fillSection("creds", setCredsHtml());
    fillSection("account", setAccountHtml(S.link === "linked"));
  }
}

/// Runs gh's browser sign-in: Beep shows the code gh prints (and copies
/// it), GitHub opens in the browser, and once the code is entered there the
/// token is saved with no further click. Cancel or a failure shows why.
export async function signInGh(redraw) {
  if (CRED.ghFlow) return;
  CRED.ghFlow = { code: "", url: "", copied: false };
  CRED.cliError = null;
  redraw();
  const onCode = new Channel();
  onCode.onmessage = async ({ code, url }) => {
    if (!CRED.ghFlow) return;
    CRED.ghFlow = { code, url, copied: false };
    redraw();
    await copy(code);
    if (CRED.ghFlow?.code === code) {
      CRED.ghFlow.copied = true;
      redraw();
    }
  };
  try {
    await invoke("gh_sign_in", { onCode });
  } catch (e) {
    CRED.cliError = { key: "github", message: String(e) };
  }
  CRED.ghFlow = null;
  CRED.ghAdd = !!CRED.cliError;
  await loadCreds();
}

/// Binds the Credentials controls inside `el`.
export function bindCreds(el) {
  const $in = sel => el.querySelector(sel);
  const redraw = () => { if (setShown()) fillSection("creds", setCredsHtml()); };
  el.querySelectorAll("[data-cred-edit]").forEach(b => b.addEventListener("click", () => {
    CRED.edit = CRED.mod?.editFor(b.dataset.credEdit, CRED.list) ?? null;
    CRED.error = "";
    redraw();
    setRoot().querySelector(CRED.edit?.kind === "forgejo" && !CRED.edit.id ? "#credUrl" : "#credSecret")?.focus();
  }));
  el.querySelectorAll("[data-cred-remove]").forEach(b => b.addEventListener("click", async () => {
    b.disabled = true;
    try {
      await invoke("remove_credential", { id: b.dataset.credRemove });
    } catch (e) {
      console.warn(e);
    }
    await loadCreds();
  }));
  // Saves the token gh or teacli here holds; the token never reaches the page.
  el.querySelectorAll("[data-cred-cli]").forEach(b => b.addEventListener("click", async () => {
    const key = b.dataset.credCli;
    const host = CRED.mod?.cliHost(key);
    if (!host || CRED.cliBusy) return;
    CRED.cliBusy = key;
    CRED.cliError = null;
    redraw();
    try {
      await invoke("use_cli_login", { host });
    } catch (e) {
      CRED.cliError = { key, message: String(e) };
    }
    CRED.cliBusy = "";
    await loadCreds();
  }));
  el.querySelectorAll("[data-cred-gh]").forEach(b => b.addEventListener("click", () => signInGh(redraw)));
  // Opens or closes "Add another GitHub account".
  el.querySelectorAll("[data-cred-gh-add]").forEach(b => b.addEventListener("click", () => {
    CRED.ghAdd = !!b.dataset.credGhAdd;
    CRED.cliError = null;
    redraw();
  }));
  // Saves the token gh here holds for one of its accounts.
  el.querySelectorAll("[data-cred-gh-use]").forEach(b => b.addEventListener("click", async () => {
    const login = b.dataset.credGhUse;
    if (!login || CRED.cliBusy) return;
    CRED.cliBusy = `gh:${login}`;
    CRED.cliError = null;
    redraw();
    try {
      await invoke("use_gh_account", { login });
      CRED.ghAdd = false;
    } catch (e) {
      CRED.cliError = { key: "github", message: String(e) };
    }
    CRED.cliBusy = "";
    await loadCreds();
  }));
  el.querySelectorAll("[data-cred-gh-default]").forEach(b => b.addEventListener("click", async () => {
    b.disabled = true;
    try {
      await invoke("set_default_github_account", { id: b.dataset.credGhDefault });
    } catch (e) {
      console.warn(e);
    }
    await loadCreds();
  }));
  el.querySelectorAll("[data-cred-gh-cancel]").forEach(b => b.addEventListener("click", () => {
    b.disabled = true;
    invoke("cancel_gh_sign_in").catch(console.warn);
  }));
  el.querySelectorAll("[data-cred-open]").forEach(a => a.addEventListener("click", e => {
    e.preventDefault();
    e.stopPropagation();
    invoke("open_url", { url: a.dataset.credOpen }).catch(console.warn);
  }));
  $in("#credCancel")?.addEventListener("click", () => { CRED.edit = null; CRED.error = ""; redraw(); });
  const save = async () => {
    if (CRED.busy || !CRED.mod) return;
    const machines = $in("#credMachines");
    if (CRED.edit && machines) CRED.edit.toMachines = machines.checked;
    const url = $in("#credUrl")?.value;
    if (CRED.edit && url !== undefined) CRED.edit.url = url;
    const d = CRED.mod.draft(CRED.edit, url, $in("#credSecret")?.value, machines?.checked);
    if (d.error) {
      CRED.error = d.error;
      const err = $in("#credErr");
      if (err) err.textContent = d.error;
      return;
    }
    CRED.busy = true;
    CRED.error = "";
    redraw();
    try {
      await invoke("save_credential", { credential: d.credential });
      if (CRED.edit?.kind === "github") CRED.ghAdd = false;
      CRED.edit = null;
    } catch (e) {
      CRED.error = String(e);
    }
    CRED.busy = false;
    // The typed token is gone from the page either way.
    await loadCreds();
  };
  $in("#credSave")?.addEventListener("click", save);
  $in("#credSecret")?.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); save(); } });
}

/// Settings, Agent accounts (`agentaccounts.js` draws it). `data` is
/// undefined until the server answers, null when it failed and false from
/// a server too old. `edit` is the agent whose token is being typed,
/// `claudeFlow` whether `claude setup-token` waits on the browser, and
/// `codex` the Codex sign-ins under way by machine id.
export const ACC = { mod: null, data: undefined, edit: null, busy: "", error: "", claudeFlow: false, claudeError: "", codex: {}, poll: null };
export const agModule = () => import("../agentaccounts.ts").then(m => (ACC.mod = m));

export function setAgentsHtml() {
  if (ACC.data === undefined || !ACC.mod) return setSkeleton(2);
  return ACC.mod.agentAccountsHtml(ACC, esc, when);
}

/// Redraws Agent accounts, unless a token is being typed there.
export function redrawAgents() {
  if (setShown() && !setRoot().querySelector("#agSecret")?.value) fillSection("agents", setAgentsHtml());
}

export async function loadAgentAccounts() {
  try {
    const [, data] = await Promise.all([agModule(), invoke("agent_accounts")]);
    ACC.data = data ?? false;
  } catch (e) {
    console.warn(e);
    ACC.data ??= null;
  }
  redrawAgents();
}

/// Asks after each Codex sign-in under way every 2 seconds, until none is.
export function pollCodex() {
  if (ACC.poll) return;
  ACC.poll = setInterval(async () => {
    const waiting = Object.entries(ACC.codex).filter(([, f]) => f.task !== undefined);
    if (!waiting.length || !setShown()) {
      clearInterval(ACC.poll);
      ACC.poll = null;
      return;
    }
    let done = false;
    for (const [id, f] of waiting) {
      try {
        const st = await invoke("codex_login_state", { machineId: id, task: f.task });
        if (st.state === "waiting") continue;
        done = true;
        if (st.state === "failed") ACC.codex[id] = { error: st.message || "Codex could not sign in." };
        else delete ACC.codex[id];
      } catch (e) {
        ACC.codex[id] = { error: String(e) };
        done = true;
      }
    }
    if (done) await loadAgentAccounts();
    else redrawAgents();
  }, 2000);
}

/// Binds the Agent accounts controls inside `el`.
export function bindAgents(el) {
  const $in = sel => el.querySelector(sel);
  el.querySelectorAll("[data-ag-edit]").forEach(b => b.addEventListener("click", () => {
    ACC.edit = b.dataset.agEdit;
    ACC.error = "";
    fillSection("agents", setAgentsHtml());
    setRoot().querySelector("#agSecret")?.focus();
  }));
  $in("#agCancel")?.addEventListener("click", () => { ACC.edit = null; ACC.error = ""; fillSection("agents", setAgentsHtml()); });
  const save = async () => {
    if (ACC.busy || !ACC.mod) return;
    const d = ACC.mod.tokenDraft(ACC.edit, $in("#agSecret")?.value);
    if (d.error) {
      ACC.error = d.error;
      const err = $in("#agErr");
      if (err) err.textContent = d.error;
      return;
    }
    ACC.busy = d.agent;
    ACC.error = "";
    try {
      await invoke("save_agent_token", { agentSlug: d.agent, secret: d.secret });
      ACC.edit = null;
    } catch (e) {
      ACC.error = String(e);
    }
    ACC.busy = "";
    // The typed token is gone from the page either way.
    const input = setRoot().querySelector("#agSecret");
    if (input) input.value = "";
    await loadAgentAccounts();
  };
  $in("#agSave")?.addEventListener("click", save);
  $in("#agSecret")?.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); save(); } });
  el.querySelectorAll("[data-ag-revoke]").forEach(b => b.addEventListener("click", async () => {
    const agent = b.dataset.agRevoke;
    ACC.busy = `revoke:${agent}`;
    b.disabled = true;
    try {
      await invoke("revoke_agent_token", { agentSlug: agent });
    } catch (e) {
      if (agent === "claude") ACC.claudeError = String(e);
      else ACC.error = String(e);
    }
    ACC.busy = "";
    await loadAgentAccounts();
  }));
  // Runs `claude setup-token` here; the token goes to the server, never the page.
  el.querySelectorAll("[data-ag-claude-sign]").forEach(b => b.addEventListener("click", async () => {
    if (ACC.claudeFlow) return;
    ACC.claudeFlow = true;
    ACC.claudeError = "";
    ACC.edit = null;
    fillSection("agents", setAgentsHtml());
    try {
      await invoke("claude_sign_in");
    } catch (e) {
      ACC.claudeError = String(e);
    }
    ACC.claudeFlow = false;
    await loadAgentAccounts();
  }));
  el.querySelectorAll("[data-ag-claude-cancel]").forEach(b => b.addEventListener("click", () => {
    b.disabled = true;
    invoke("cancel_claude_sign_in").catch(console.warn);
  }));
  // Starts Codex's device sign-in on that machine, copies the code and opens the page.
  el.querySelectorAll("[data-ag-codex-sign]").forEach(b => b.addEventListener("click", async () => {
    const id = b.dataset.agCodexSign;
    if (ACC.codex[id]?.starting || ACC.codex[id]?.task !== undefined) return;
    ACC.codex[id] = { starting: true };
    redrawAgents();
    try {
      const flow = await invoke("codex_login_start", { machineId: id });
      ACC.codex[id] = { ...flow, copied: false };
      redrawAgents();
      pollCodex();
      await copy(flow.code);
      if (ACC.codex[id]?.code === flow.code) ACC.codex[id].copied = true;
      invoke("open_url", { url: flow.url }).catch(console.warn);
    } catch (e) {
      ACC.codex[id] = { error: String(e) };
    }
    redrawAgents();
  }));
  el.querySelectorAll("[data-ag-codex-cancel]").forEach(b => b.addEventListener("click", async () => {
    const id = b.dataset.agCodexCancel;
    const f = ACC.codex[id];
    b.disabled = true;
    if (f?.task !== undefined) await invoke("codex_login_cancel", { machineId: id, task: f.task }).catch(console.warn);
    delete ACC.codex[id];
    redrawAgents();
  }));
  el.querySelectorAll("[data-ag-copy]").forEach(b => b.addEventListener("click", () => copy(b.dataset.agCopy, b)));
  el.querySelectorAll("[data-ag-open]").forEach(b => b.addEventListener("click", () =>
    invoke("open_url", { url: b.dataset.agOpen }).catch(console.warn)));
}

