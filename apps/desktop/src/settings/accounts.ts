import { CredentialsView, type CredentialActions } from "./credentials-view.tsx";
import { createElement } from "react";
import { AgentAccountsView, type AgentAccountActions } from "./agentaccounts-view.tsx";
import { subscribeChanges as subscribeTopics, topic } from "../ui/mesh-events.ts";
import type * as AgentAccountsModule from "./agentaccounts.ts";
import type { AgentAccounts, AgentAccountsState, CodexFlow, CodexLogin } from "./agentaccounts.ts";
import type * as CredentialsModule from "./credentials.ts";
import type { CliLogins, CredCheck, Credential, CredState, GhFlow, GitHubAccount } from "./credentials.ts";
import { when } from "../chat/chats.ts";
import { copy } from "../ui/parts.ts";
import type { Machine } from "../machines/machines.ts";
import { SET, setShown, type SettingsContentOwner } from "./settings.ts";
import { Channel, invoke } from "../ui/tauri.ts";

interface AccountMount { host: HTMLElement; owner: SettingsContentOwner; observer: MutationObserver }

/// Settings, Credentials (`credentials.ts` draws it, loaded on first use into
/// `mod`).
export const CRED: CredState & { mod: typeof CredentialsModule | null } = { mod: null, list: undefined, github: undefined, edit: null, busy: false, error: "", cli: undefined, cliBusy: "", cliError: null, ghFlow: null, ghAdd: false, checks: {} };
const credModule = () => import("./credentials.ts").then(m => (CRED.mod = m));

const redrawCreds = () => { if (setShown()) renderCreds(); };

/// Asks gh and teacli on this Mac what they are signed in to, then redraws.
async function loadCliLogins() {
  try {
    CRED.cli = await invoke<CliLogins>("cli_logins");
  } catch (e) {
    console.warn(e);
    CRED.cli = null;
  }

  redrawCreds();
}

/// Checks each saved Forgejo token the server could not check when it was
/// saved, once per token: from the server, else from a machine with teacli.
function checkForgejo(list: Credential[]) {
  const checks = (CRED.checks ??= {});

  for (const c of list) {
    if (c.kind !== "forgejo" || c.unreadable || c.account || checks[c.id]) continue;
    checks[c.id] = { state: "checking" };
    invoke<CredCheck>("check_credential", { id: c.id })
      .then(check => { checks[c.id] = check; })
      .catch(e => { console.warn(e); delete checks[c.id]; })
      .finally(redrawCreds);
  }
}

/// Lists the credentials again and redraws their section.
export async function loadCreds() {
  loadCliLogins();

  try {
    // A server from before several GitHub accounts does not list them:
    // the credentials list then has the one GitHub token.
    const accounts = invoke<GitHubAccount[]>("list_github_accounts").catch(e => { console.warn(e); return undefined; });
    const [, list, github] = await Promise.all([credModule(), invoke<Credential[]>("list_credentials"), accounts]);
    CRED.list = list;
    CRED.github = github;
    checkForgejo(list);
  } catch (e) {
    console.warn(e);
    CRED.list ??= null;
  }

  redrawCreds();
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

let credsRoot: AccountMount | null = null;
let credEditVersion = 0;
export function preserveCreds() {
  const host = credsRoot?.host;
  if (!host?.isConnected) return () => {};
  const active = host.contains(document.activeElement) ? document.activeElement : null;
  const input = active instanceof HTMLInputElement ? active : null;
  const start = input?.selectionStart ?? null, end = input?.selectionEnd ?? null;
  host.remove();
  return (container?: Element) => {
    const replacement = container?.querySelector("#set-creds");
    if (!replacement) { disposeCreds(); return; }
    replacement.replaceWith(host);
    if (active instanceof HTMLElement) {
      active.focus({ preventScroll: true });
      if (input && start !== null && end !== null) input.setSelectionRange(start, end);
    }
  };
}
export function disposeCreds() {
  const current = credsRoot;
  credsRoot = null;
  if (!current) return;
  credEditVersion++;
  current.observer.disconnect();
  current.owner.remove("credentials");
}
export function renderCreds() {
  if (!credsRoot?.host.isConnected) return;
  const current = credsRoot, machines = Array.isArray(SET.machines) ? SET.machines : null;
  current.owner.show("credentials", current.host, createElement(CredentialsView, { state: CRED, machines, actions: credActions }));
}
export function bindCreds(el: Element, owner: SettingsContentOwner) {
  const host = el.matches("#set-creds") ? el : el.querySelector("#set-creds");
  if (!(host instanceof HTMLElement)) return;
  if (credsRoot?.host === host) { renderCreds(); return; }
  disposeCreds();
  const observer = new MutationObserver(() => { if (credsRoot?.host === host && !host.isConnected) disposeCreds(); });
  observer.observe(document.body, { childList: true, subtree: true });
  credsRoot = { host, owner, observer };
  renderCreds();
}
async function useLogin(busy: string, errorKey: string, act: () => Promise<unknown>) {
  if (CRED.cliBusy) return;
  CRED.cliBusy = busy; CRED.cliError = null; redrawCreds();
  try { await act(); } catch (error) { CRED.cliError = { key: errorKey, message: String(error) }; }
  CRED.cliBusy = ""; await loadCreds();
}
async function credentialCommand(command: string, args: Record<string, unknown>) {
  try { await invoke(command, args); } catch (error) { console.warn(error); }
  await loadCreds();
}
const credActions: CredentialActions = {
  edit(key) { credEditVersion++; CRED.edit = CRED.mod?.editFor(key, CRED.list) ?? null; CRED.error = ""; redrawCreds(); },
  cancel() { credEditVersion++; CRED.edit = null; CRED.error = ""; redrawCreds(); },
  async save(url, secret) {
    if (CRED.busy || !CRED.mod) return;
    if (CRED.edit?.kind === "forgejo" && !CRED.edit.id) CRED.edit.url = url;
    const draft = CRED.mod.draft(CRED.edit, url, secret);
    if (draft.error) { CRED.error = draft.error; redrawCreds(); return; }
    const version = credEditVersion, kind = CRED.edit?.kind;
    CRED.busy = true; CRED.error = ""; redrawCreds();
    try {
      await invoke("save_credential", { credential: draft.credential });
      if (version === credEditVersion) { if (kind === "github") CRED.ghAdd = false; CRED.edit = null; }
    } catch (error) { if (version === credEditVersion) CRED.error = String(error); }
    CRED.busy = false;
    if (version === credEditVersion) { const input = credsRoot?.host.querySelector<HTMLInputElement>("#credSecret"); if (input) input.value = ""; }
    await loadCreds();
  },
  remove(id) { return credentialCommand("remove_credential", { id }); },
  useCli(key) { const host = CRED.mod?.cliHost(key); if (host) void useLogin(key, key, () => invoke("use_cli_login", { host })); },
  signIn() { void signInGh(redrawCreds); },
  cancelSignIn() { invoke("cancel_gh_sign_in").catch(console.warn); },
  addAccount(open) { CRED.ghAdd = open; CRED.cliError = null; redrawCreds(); },
  useAccount(login) { void useLogin(`gh:${login}`, "github", async () => { await invoke("use_gh_account", { login }); CRED.ghAdd = false; }); },
  defaultAccount(id) { return credentialCommand("set_default_github_account", { id }); },
  open(url) { invoke("open_url", { url }).catch(console.warn); },
};

/// Settings, Agent accounts (`agentaccounts.ts` draws it, loaded on first use
/// into `mod`).
const ACC: AgentAccountsState & {
  mod: typeof AgentAccountsModule | null;
  busy: string;
  codex: Record<string, CodexFlow>;
  poll: (() => void) | null;
} = { mod: null, data: undefined, edit: null, busy: "", error: "", claudeFlow: false, claudeError: "", codex: {}, poll: null };
const agModule = () => import("./agentaccounts.ts").then(m => (ACC.mod = m));

/// Redraws Agent accounts, unless a token is being typed there.
function redrawAgents() {
  if (setShown()) renderAgents();
}

export async function loadAgentAccounts() {
  try {
    const [, data, machines] = await Promise.all([
      agModule(),
      invoke<AgentAccounts | null>("agent_accounts"),
      invoke<Machine[]>("list_machines").catch((): Machine[] => []),
    ]);
    ACC.data = data ?? false;
    // Each machine's own Claude Code sign-in, as it last reported it.
    if (data) data.claude_logins = new Map(machines.flatMap(m => {
      const login = m.env?.logins?.find(l => l.tool === "claude");
      return login ? [[m.id, login] as const] : [];
    }));
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

/// Sign-in tasks announce output and completion over the node stream.
function pollCodex() {
  if (ACC.poll) return;
  const check = async () => {
    const waiting = Object.entries(ACC.codex).filter(([, f]) => f.task !== undefined);

    if (!waiting.length || !setShown()) {
      ACC.poll?.();
      ACC.poll = null;
      return;
    }

    let done = false;

    for (const [id, f] of waiting) {
      try {
        const st = await invoke<CodexState>("codex_login_state", { machineId: id, task: f.task });
        if (ACC.codex[id] !== f || st.state === "waiting") continue;
        done = true;
        if (st.state === "failed") ACC.codex[id] = { error: st.message || "Codex could not sign in." };
        else delete ACC.codex[id];
      } catch (e) {
        if (ACC.codex[id] !== f) continue;
        ACC.codex[id] = { error: String(e) };
        done = true;
      }
    }

    if (done) await loadAgentAccounts();
    else redrawAgents();
  };
  ACC.poll = subscribeTopics([topic("processes"), topic("process-output"), topic("accounts")], () => check(), null, setShown);
  void check();
}

let agentsRoot: AccountMount | null = null;
let agentEditVersion = 0;
export function preserveAgents() {
  const host = agentsRoot?.host;
  if (!host?.isConnected) return () => {};
  const active = host.contains(document.activeElement) ? document.activeElement : null;
  const input = active instanceof HTMLInputElement ? active : null;
  const start = input?.selectionStart ?? null, end = input?.selectionEnd ?? null;
  host.remove();
  return (container?: Element) => {
    const replacement = container?.querySelector("#set-agents");
    if (!replacement) { disposeAgents(); return; }
    replacement.replaceWith(host);
    if (active instanceof HTMLElement) {
      active.focus({ preventScroll: true });
      if (input && start !== null && end !== null) input.setSelectionRange(start, end);
    }
  };
}
export function disposeAgents() {
  const current = agentsRoot;
  agentsRoot = null;
  if (!current) return;
  agentEditVersion++;
  current.observer.disconnect();
  current.owner.remove("agent-accounts");
}
function renderAgents() {
  if (!agentsRoot?.host.isConnected) return;
  const current = agentsRoot;
  current.owner.show("agent-accounts", current.host, createElement(AgentAccountsView, { state: ACC, ago: when, now: Date.now(), actions: agentActions }));
}
export function bindAgents(el: Element, owner: SettingsContentOwner) {
  const host = el.matches("#set-agents") ? el : el.querySelector("#set-agents");
  if (!(host instanceof HTMLElement)) return;
  if (agentsRoot?.host === host) { renderAgents(); return; }
  disposeAgents();
  const observer = new MutationObserver(() => { if (agentsRoot?.host === host && !host.isConnected) disposeAgents(); });
  observer.observe(document.body, { childList: true, subtree: true });
  agentsRoot = { host, owner, observer };
  renderAgents();
}
const agentActions: AgentAccountActions = {
  edit(agent) { agentEditVersion++; ACC.edit = agent; ACC.error = ""; renderAgents(); },
  cancel() { agentEditVersion++; ACC.edit = null; ACC.error = ""; renderAgents(); },
  async save(secret) {
    if (ACC.busy || !ACC.mod) return;
    const draft = ACC.mod.tokenDraft(ACC.edit, secret);
    if (!draft.agent) { ACC.error = draft.error; renderAgents(); return; }
    const version = agentEditVersion;
    ACC.busy = draft.agent; ACC.error = ""; renderAgents();
    try { await invoke("save_agent_token", { agentSlug: draft.agent, secret: draft.secret }); if (version === agentEditVersion) ACC.edit = null; }
    catch (error) { if (version === agentEditVersion) ACC.error = String(error); }
    ACC.busy = "";
    const input = agentsRoot?.host.querySelector<HTMLInputElement>("#agSecret");
    if (input && version === agentEditVersion) input.value = "";
    await loadAgentAccounts();
  },
  async revoke(agent) {
    ACC.busy = `revoke:${agent}`; renderAgents();
    try { await invoke("revoke_agent_token", { agentSlug: agent }); }
    catch (error) { if (agent === "claude") ACC.claudeError = String(error); else ACC.error = String(error); }
    ACC.busy = ""; await loadAgentAccounts();
  },
  async claudeSign() {
    if (ACC.claudeFlow) return;
    agentEditVersion++; ACC.claudeFlow = true; ACC.claudeError = ""; ACC.edit = null; renderAgents();
    try { await invoke("claude_sign_in"); } catch (error) { ACC.claudeError = String(error); }
    ACC.claudeFlow = false; await loadAgentAccounts();
  },
  claudeCancel() { invoke("cancel_claude_sign_in").catch(console.warn); },
  async codexSign(id) {
    if (ACC.codex[id]?.starting || ACC.codex[id]?.task !== undefined) return;
    ACC.codex[id] = { starting: true }; redrawAgents();
    try {
      const flow = await invoke<CodexLogin>("codex_login_start", { machineId: id });
      ACC.codex[id] = { ...flow, copied: false }; redrawAgents(); pollCodex();
      await copy(flow.code);
      const now = ACC.codex[id];
      if (now?.task === flow.task) { now.copied = true; invoke("open_url", { url: flow.url }).catch(console.warn); }
    } catch (error) { ACC.codex[id] = { error: String(error) }; }
    redrawAgents();
  },
  async codexCancel(id) {
    const flow = ACC.codex[id];
    if (flow?.task !== undefined) await invoke("codex_login_cancel", { machineId: id, task: flow.task }).catch(console.warn);
    delete ACC.codex[id]; redrawAgents();
  },
  copy(code, button) { void copy(code, button); },
  open(url) { invoke("open_url", { url }).catch(console.warn); },
};
