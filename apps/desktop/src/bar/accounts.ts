import type * as AgentAccountsModule from "../agentaccounts.ts";
import type { AgentAccounts, AgentAccountsState, CodexFlow, CodexLogin } from "../agentaccounts.ts";
import type * as CredentialsModule from "../credentials.ts";
import type { CliLogins, Credential, CredState, GhFlow, GitHubAccount, Provider } from "../credentials.ts";
import { copy, when } from "./chats.ts";
import { Channel, esc, invoke, S } from "./core.ts";
import { fillSection, SET, setAccountHtml, setRoot, setShown, setSkeleton } from "./settings.ts";

/// Settings, Credentials (`credentials.ts` draws it, loaded on first use
/// into `mod`). `list` is undefined until the server answers and null when
/// it failed; `edit` is the row whose token is being typed. The rest is as
/// `CredState` says.
export const CRED: CredState & { mod: typeof CredentialsModule | null } = { mod: null, list: undefined, github: undefined, providers: [], edit: null, busy: false, error: "", cli: undefined, cliBusy: "", cliError: null, ghFlow: null, ghAdd: false };
const credModule = () => import("../credentials.ts").then(m => (CRED.mod = m));

/// Finds a control inside `el`, the section or page being bound.
type Find = <T extends Element = HTMLElement>(sel: string) => T | null;
const finder = (el: Element): Find => sel => el.querySelector(sel);
/// Runs `fn` on each element inside `el` that has the attribute `attr`.
const eachIn = (el: Element) => <T extends HTMLElement = HTMLButtonElement>(attr: string, fn: (b: T) => void) =>
  el.querySelectorAll<T>(`[${attr}]`).forEach(fn);
/// The value typed in the input `sel` names on the Settings page, if any.
const typed = (sel: string) => setRoot().querySelector<HTMLInputElement>(sel)?.value;

export function setCredsHtml() {
  if (CRED.list === undefined || !CRED.mod) return setSkeleton(2);
  if (CRED.list === null) return `<div class="dmuted">Could not load your credentials.</div>`;
  // Which machines have gh and teacli, when the machines are known.
  const machines = Array.isArray(SET.machines) ? SET.machines : null;
  return CRED.mod.credentialsHtml(CRED, machines, esc, when);
}

/// Asks gh and teacli on this Mac what they are signed in to, then redraws.
async function loadCliLogins() {
  try {
    CRED.cli = await invoke<CliLogins>("cli_logins");
  } catch (e) {
    console.warn(e);
    CRED.cli = null;
  }
  // Not while a token is typed: the redraw would drop it.
  if (setShown() && CRED.list && !typed("#credSecret")) fillSection("creds", setCredsHtml());
}

/// Lists the credentials again and redraws their section.
export async function loadCreds() {
  loadCliLogins();
  try {
    // A server from before several GitHub accounts does not list them:
    // the credentials list then has the one GitHub token.
    const accounts = invoke<GitHubAccount[]>("list_github_accounts").catch(e => { console.warn(e); return undefined; });
    const [, list, providers, github] = await Promise.all([credModule(), invoke<Credential[]>("list_credentials"), invoke<Provider[]>("credential_providers"), accounts]);
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
async function signInGh(redraw: () => void) {
  if (CRED.ghFlow) return;
  CRED.ghFlow = { code: "", url: "", copied: false };
  CRED.cliError = null;
  redraw();
  const onCode = new Channel<GhFlow>();
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

/// Runs `act` for a row's button: the button stays disabled until the
/// section redraws, and a failure only goes to the console.
const settle = (b: HTMLButtonElement, act: () => Promise<unknown>, after: () => Promise<unknown>) => async () => {
  b.disabled = true;
  try {
    await act();
  } catch (e) {
    console.warn(e);
  }
  await after();
};

/// Binds the Credentials controls inside `el`.
export function bindCreds(el: Element) {
  const $in = finder(el);
  const each = eachIn(el);
  const redraw = () => { if (setShown()) fillSection("creds", setCredsHtml()); };
  each("data-cred-edit", b => b.addEventListener("click", () => {
    CRED.edit = CRED.mod?.editFor(b.dataset.credEdit!, CRED.list) ?? null;
    CRED.error = "";
    redraw();
    setRoot().querySelector<HTMLElement>(CRED.edit?.kind === "forgejo" && !CRED.edit.id ? "#credUrl" : "#credSecret")?.focus();
  }));
  each("data-cred-remove", b => b.addEventListener("click", settle(b, () => invoke("remove_credential", { id: b.dataset.credRemove }), loadCreds)));
  // Saves the sign-in gh or teacli here holds, under `busy`; the token
  // never reaches the page.
  const useLogin = async (busy: string, errorKey: string, act: () => Promise<unknown>) => {
    CRED.cliBusy = busy;
    CRED.cliError = null;
    redraw();
    try {
      await act();
    } catch (e) {
      CRED.cliError = { key: errorKey, message: String(e) };
    }
    CRED.cliBusy = "";
    await loadCreds();
  };
  each("data-cred-cli", b => b.addEventListener("click", async () => {
    const key = b.dataset.credCli!;
    const host = CRED.mod?.cliHost(key);
    if (!host || CRED.cliBusy) return;
    await useLogin(key, key, () => invoke("use_cli_login", { host }));
  }));
  each("data-cred-gh", b => b.addEventListener("click", () => signInGh(redraw)));
  // Opens or closes "Add another GitHub account".
  each("data-cred-gh-add", b => b.addEventListener("click", () => {
    CRED.ghAdd = !!b.dataset.credGhAdd;
    CRED.cliError = null;
    redraw();
  }));
  // Saves the token gh here holds for one of its accounts.
  each("data-cred-gh-use", b => b.addEventListener("click", async () => {
    const login = b.dataset.credGhUse;
    if (!login || CRED.cliBusy) return;
    await useLogin(`gh:${login}`, "github", async () => {
      await invoke("use_gh_account", { login });
      CRED.ghAdd = false;
    });
  }));
  each("data-cred-gh-default", b => b.addEventListener("click", settle(b, () => invoke("set_default_github_account", { id: b.dataset.credGhDefault }), loadCreds)));
  each("data-cred-gh-cancel", b => b.addEventListener("click", () => {
    b.disabled = true;
    invoke("cancel_gh_sign_in").catch(console.warn);
  }));
  each<HTMLAnchorElement>("data-cred-open", a => a.addEventListener("click", e => {
    e.preventDefault();
    e.stopPropagation();
    invoke("open_url", { url: a.dataset.credOpen }).catch(console.warn);
  }));
  $in("#credCancel")?.addEventListener("click", () => { CRED.edit = null; CRED.error = ""; redraw(); });
  const save = async () => {
    if (CRED.busy || !CRED.mod) return;
    const machines = $in<HTMLInputElement>("#credMachines");
    // The checkbox shows only in an API key's form, the URL only in a new server's.
    if (CRED.edit?.kind === "api_key" && machines) CRED.edit.toMachines = machines.checked;
    const url = $in<HTMLInputElement>("#credUrl")?.value;
    if (CRED.edit?.kind === "forgejo" && url !== undefined) CRED.edit.url = url;
    const d = CRED.mod.draft(CRED.edit, url, $in<HTMLInputElement>("#credSecret")?.value, machines?.checked);
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

/// Settings, Agent accounts (`agentaccounts.ts` draws it, loaded on first
/// use into `mod`). `data` is undefined until the server answers, null when
/// it failed and false from a server too old. `edit` is the agent whose
/// token is being typed, `claudeFlow` whether `claude setup-token` waits on
/// the browser, `codex` the Codex sign-ins under way by machine id and
/// `poll` the timer asking after them.
export const ACC: AgentAccountsState & {
  mod: typeof AgentAccountsModule | null;
  busy: string;
  codex: Record<string, CodexFlow>;
  poll: ReturnType<typeof setInterval> | null;
} = { mod: null, data: undefined, edit: null, busy: "", error: "", claudeFlow: false, claudeError: "", codex: {}, poll: null };
const agModule = () => import("../agentaccounts.ts").then(m => (ACC.mod = m));

export function setAgentsHtml() {
  if (ACC.data === undefined || !ACC.mod) return setSkeleton(2);
  return ACC.mod.agentAccountsHtml(ACC, esc, when);
}

/// Redraws Agent accounts, unless a token is being typed there.
function redrawAgents() {
  if (setShown() && !typed("#agSecret")) fillSection("agents", setAgentsHtml());
}

export async function loadAgentAccounts() {
  try {
    const [, data] = await Promise.all([agModule(), invoke<AgentAccounts | null>("agent_accounts")]);
    ACC.data = data ?? false;
  } catch (e) {
    console.warn(e);
    ACC.data ??= null;
  }
  redrawAgents();
}

/// How a Codex sign-in stands (CodexStateView).
interface CodexState {
  state: "waiting" | "signed_in" | "failed" | "gone";
  message: string | null;
}

/// Asks after each Codex sign-in under way every 2 seconds, until none is.
function pollCodex() {
  if (ACC.poll) return;
  ACC.poll = setInterval(async () => {
    const waiting = Object.entries(ACC.codex).filter(([, f]) => f.task !== undefined);
    if (!waiting.length || !setShown()) {
      clearInterval(ACC.poll!);
      ACC.poll = null;
      return;
    }
    let done = false;
    for (const [id, f] of waiting) {
      try {
        const st = await invoke<CodexState>("codex_login_state", { machineId: id, task: f.task });
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
export function bindAgents(el: Element) {
  const $in = finder(el);
  const each = eachIn(el);
  const draw = () => fillSection("agents", setAgentsHtml());
  each("data-ag-edit", b => b.addEventListener("click", () => {
    ACC.edit = b.dataset.agEdit;
    ACC.error = "";
    draw();
    setRoot().querySelector<HTMLElement>("#agSecret")?.focus();
  }));
  $in("#agCancel")?.addEventListener("click", () => { ACC.edit = null; ACC.error = ""; draw(); });
  const save = async () => {
    if (ACC.busy || !ACC.mod) return;
    const d = ACC.mod.tokenDraft(ACC.edit, $in<HTMLInputElement>("#agSecret")?.value);
    // A draft names its agent unless it is an error.
    if (!d.agent) {
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
    const input = setRoot().querySelector<HTMLInputElement>("#agSecret");
    if (input) input.value = "";
    await loadAgentAccounts();
  };
  $in("#agSave")?.addEventListener("click", save);
  $in("#agSecret")?.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); save(); } });
  each("data-ag-revoke", b => b.addEventListener("click", async () => {
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
  each("data-ag-claude-sign", b => b.addEventListener("click", async () => {
    if (ACC.claudeFlow) return;
    ACC.claudeFlow = true;
    ACC.claudeError = "";
    ACC.edit = null;
    draw();
    try {
      await invoke("claude_sign_in");
    } catch (e) {
      ACC.claudeError = String(e);
    }
    ACC.claudeFlow = false;
    await loadAgentAccounts();
  }));
  each("data-ag-claude-cancel", b => b.addEventListener("click", () => {
    b.disabled = true;
    invoke("cancel_claude_sign_in").catch(console.warn);
  }));
  // Starts Codex's device sign-in on that machine, copies the code and opens the page.
  each("data-ag-codex-sign", b => b.addEventListener("click", async () => {
    const id = b.dataset.agCodexSign!;
    if (ACC.codex[id]?.starting || ACC.codex[id]?.task !== undefined) return;
    ACC.codex[id] = { starting: true };
    redrawAgents();
    try {
      const flow = await invoke<CodexLogin>("codex_login_start", { machineId: id });
      ACC.codex[id] = { ...flow, copied: false };
      redrawAgents();
      pollCodex();
      await copy(flow.code);
      const now = ACC.codex[id];
      if (now?.code === flow.code) now.copied = true;
      invoke("open_url", { url: flow.url }).catch(console.warn);
    } catch (e) {
      ACC.codex[id] = { error: String(e) };
    }
    redrawAgents();
  }));
  each("data-ag-codex-cancel", b => b.addEventListener("click", async () => {
    const id = b.dataset.agCodexCancel!;
    const f = ACC.codex[id];
    b.disabled = true;
    if (f?.task !== undefined) await invoke("codex_login_cancel", { machineId: id, task: f.task }).catch(console.warn);
    delete ACC.codex[id];
    redrawAgents();
  }));
  each("data-ag-copy", b => b.addEventListener("click", () => copy(b.dataset.agCopy!, b)));
  each("data-ag-open", b => b.addEventListener("click", () =>
    invoke("open_url", { url: b.dataset.agOpen }).catch(console.warn)));
}
