import { CredentialsView, type CredentialActions } from "./credentials-view.tsx";
import { AgentAccountsView, type AgentAccountActions } from "./agentaccounts-view.tsx";
import { subscribeChanges, subscribeEach, topic } from "../ui/mesh-events.ts";
import { accountName, codexTaskRunning, tokenDraft } from "./agentaccounts.ts";
import type { AgentAccounts, AgentAccountsState, CodexFlow, CodexLogin } from "./agentaccounts.ts";
import { cliHost, draft as credentialDraft, editFor } from "./credentials.ts";
import type { ApiProvider, CliLogins, CredCheck, Credential, CredState, GhFlow, GitHubAccount } from "./credentials.ts";
import { copy } from "../ui/clipboard.ts";
import type { Machine } from "../machines/machines.tsx";
import { ensureMachines, MACHINES } from "../machines/machine-store.ts";
import { openSignIn } from "../machines/signin.tsx";
import { redrawSettings, setShown } from "./settings.tsx";
import { Mounted } from "./settings-shell-view.tsx";
import { Channel, invoke } from "../ui/tauri.ts";
import { AMBIENT_MS, onShownChange } from "../ui/active.ts";
import { pushedView } from "../ui/snapshot-state.ts";
import { formatAgo } from "../ui/format.ts";

/// Settings, Credentials (`credentials.ts` holds its logic).
export const CRED: CredState = { list: undefined, github: undefined, edit: null, busy: false, error: "", cli: undefined, cliBusy: "", cliError: null, ghFlow: null, ghAdd: false, checks: {} };

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

/// Checks each saved Forgejo token Beep could not check when it was saved,
/// once per token: from this device, else from a machine with teacli.
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
    // When the accounts cannot be read, the credentials list has the one
    // GitHub token.
    const accounts = invoke<GitHubAccount[]>("list_github_accounts").catch(e => { console.warn(e); return undefined; });
    const providers = CRED.providers ? Promise.resolve(CRED.providers) : invoke<ApiProvider[]>("api_providers");
    const [list, github, known] = await Promise.all([invoke<Credential[]>("list_credentials"), accounts, providers]);
    CRED.list = list;
    CRED.github = github;
    CRED.providers = known;
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

// Credentials render inside the Settings owner. Leaving drops what a pending save would write back.
let credsShown = false;
let credEditVersion = 0;
const credsEntered = () => { credsShown = true; };
const credsLeft = () => { credsShown = false; credEditVersion++; };
export function credentialsContent() {
  const machines = MACHINES.list;
  return <Mounted entered={credsEntered} left={credsLeft}><CredentialsView state={CRED} machines={machines} actions={credActions} /></Mounted>;
}
export function renderCreds() {
  if (credsShown) redrawSettings();
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
  edit(key) { credEditVersion++; CRED.edit = editFor(key, CRED.list); CRED.error = ""; redrawCreds(); },
  cancel() { credEditVersion++; CRED.edit = null; CRED.error = ""; redrawCreds(); },
  async save(url, secret, clear) {
    if (CRED.busy) return;
    if (CRED.edit?.kind === "forgejo" && !CRED.edit.id) CRED.edit.url = url;
    const draft = credentialDraft(CRED.edit, url, secret);
    if (draft.error) { CRED.error = draft.error; redrawCreds(); return; }
    const version = credEditVersion, kind = CRED.edit?.kind;
    CRED.busy = true; CRED.error = ""; redrawCreds();
    try {
      await invoke("save_credential", { credential: draft.credential });
      if (version === credEditVersion) { if (kind === "github") CRED.ghAdd = false; CRED.edit = null; }
    } catch (error) { if (version === credEditVersion) CRED.error = String(error); }
    CRED.busy = false;
    if (version === credEditVersion) clear();
    await loadCreds();
  },
  remove(id) { return credentialCommand("remove_credential", { id }); },
  useCli(key) { const host = cliHost(key); if (host) void useLogin(key, key, () => invoke("use_cli_login", { host })); },
  signIn() { void signInGh(redrawCreds); },
  cancelSignIn() { invoke("cancel_gh_sign_in").catch(console.warn); },
  addAccount(open) { CRED.ghAdd = open; CRED.cliError = null; redrawCreds(); },
  useAccount(login) { void useLogin(`gh:${login}`, "github", async () => { await invoke("use_gh_account", { login }); CRED.ghAdd = false; }); },
  defaultAccount(id) { return credentialCommand("set_default_github_account", { id }); },
  open(url) { invoke("open_url", { url }).catch(console.warn); },
};

/// Settings, Agent accounts (`agentaccounts.ts` holds its logic).
const ACC: AgentAccountsState & {
  busy: string;
  codex: Record<string, CodexFlow>;
  poll: { stop: () => void; check: () => Promise<void> } | null;
} = { data: undefined, edit: null, busy: "", error: "", claudeFlow: false, claudeError: "", codex: {}, poll: null };

/// Redraws Agent accounts, unless a token is being typed there.
function redrawAgents() {
  if (setShown()) renderAgents();
}

/// Takes `data` as the accounts, with each machine's own sign-in as it last reported it.
function takeAccounts(data: AgentAccounts, machines: readonly Machine[]) {
  ACC.data = data;
  if (!data) return;

  data.claude_logins = new Map(machines.flatMap(m => {
    const login = m.env?.logins?.find(l => l.tool === "claude");
    return login ? [[m.id, login] as const] : [];
  }));
  data.agy_machines = machines.flatMap(m => {
    const login = m.env?.logins?.find(l => l.tool === "agy");
    return login ? [{ id: m.id, name: m.name, online: m.online, signed_in: login.signed_in }] : [];
  });
}

/// Reads the accounts, for the first draw and after a sign-in on a machine. Tokens saved or revoked here
/// come back as a push.
export async function loadAgentAccounts() {
  try {
    const [data, machines] = await Promise.all([
      invoke<AgentAccounts>("agent_accounts"),
      ensureMachines().catch((): Machine[] => []),
    ]);
    takeAccounts(data, machines);
  } catch (e) {
    console.warn(e);
    ACC.data ??= null;
  }

  redrawAgents();
}

/// The accounts the push last gave, read when none came yet.
export function ensureAgentAccounts() {
  if (!ACC.data) void loadAgentAccounts();
}

// A machines change moves the sign-ins drawn beside the accounts, so it takes them again.
subscribeChanges([topic("accounts"), topic("machines")], { deliver: "each", receive: event => {
  const pushed = pushedView<AgentAccounts>(event.data, "agent_accounts");

  if (pushed && "error" in pushed) { console.warn("agent accounts:", pushed.error); return; }
  if (pushed) takeAccounts(pushed.value, MACHINES.list ?? []);
  else if (ACC.data) takeAccounts(ACC.data, MACHINES.list ?? []);
  else return;

  redrawAgents();
} }, { wait: AMBIENT_MS });

/// How a Codex sign-in stands (CodexStateView).
interface CodexState {
  state: "waiting" | "signed_in" | "failed" | "gone";
  message: string | null;
}

/// Sign-in tasks announce completion over the node stream, which is followed only while one
/// runs. Settings off screen hears nothing, so showing it again checks once.
function followCodex() {
  if (ACC.poll) return;
  /// Asks for the final state of the sign-ins of `only`, or of every machine that waits.
  const check = async (only?: string) => {
    const waiting = Object.entries(ACC.codex).filter(([id, f]) => f.task !== undefined && (only === undefined || id === only));

    if (!Object.values(ACC.codex).some(f => f.task !== undefined)) {
      ACC.poll?.stop();
      ACC.poll = null;
      return;
    }

    if (!waiting.length || !setShown()) return;

    // The machines are asked together: a slow one does not hold the others.
    const answers = await Promise.all(waiting.map(async ([id, f]): Promise<{ id: string; f: typeof f; st: CodexState } | { id: string; f: typeof f; error: string }> => {
      try {
        return { id, f, st: await invoke<CodexState>("codex_login_state", { machineId: id, task: f.task }) };
      } catch (e) {
        return { id, f, error: String(e) };
      }
    }));
    for (const answer of answers) {
      const { id, f } = answer;
      if (ACC.codex[id] !== f) continue;

      if ("error" in answer) {
        ACC.codex[id] = { error: answer.error };
      } else if (answer.st.state !== "waiting") {
        if (answer.st.state === "failed") ACC.codex[id] = { error: answer.st.message || "Codex could not sign in." };
        else delete ACC.codex[id];
      }
    }

    redrawAgents();
  };
  // The node's task list says while the sign-in runs; once it has ended or is gone, its state is asked once.
  const stop = subscribeEach(topic("processes"), event => {
    const f = ACC.codex[event.machine_id];
    if (f?.task !== undefined && codexTaskRunning(event.data, f.task) === false) return check(event.machine_id);
  }, { when: setShown });
  ACC.poll = { stop, check: () => check() };
  void check();
}

if (typeof document !== "undefined") onShownChange(shown => { if (shown) void ACC.poll?.check(); });

let agentsShown = false;
let agentEditVersion = 0;
const agentsEntered = () => { agentsShown = true; void ACC.poll?.check(); };
const agentsLeft = () => { agentsShown = false; agentEditVersion++; };
export function agentAccountsContent() {
  return <Mounted entered={agentsEntered} left={agentsLeft}><AgentAccountsView state={ACC} ago={ms => formatAgo(ms, "chat")} now={Date.now()} actions={agentActions} /></Mounted>;
}
function renderAgents() {
  if (agentsShown) redrawSettings();
}
const agentActions: AgentAccountActions = {
  add(agent) { agentEditVersion++; ACC.add = agent ? { agent, error: "" } : null; ACC.edit = null; renderAgents(); },
  async addSave(name, secret) {
    const add = ACC.add;
    if (ACC.busy || !add) return;
    const named = accountName(name), draft = tokenDraft(add.agent, secret);
    if (named.name === null || !draft.agent) { add.error = named.error ?? draft.error ?? ""; renderAgents(); return; }
    if (draft.agent === "codex" && draft.secret.startsWith("sk-")) { add.error = "A named Codex account takes a ChatGPT personal access token. An API key goes in Codex's own row above."; renderAgents(); return; }
    ACC.busy = "add"; add.error = ""; renderAgents();
    try { await invoke("save_agent_token", { agentSlug: draft.agent, name: named.name, secret: draft.secret }); if (ACC.add === add) ACC.add = null; }
    catch (error) { add.error = String(error); }
    ACC.busy = ""; redrawAgents();
  },
  async addSign(name) {
    const add = ACC.add;
    if (ACC.claudeFlow || !add) return;
    const named = accountName(name);
    if (named.name === null) { add.error = named.error; renderAgents(); return; }
    ACC.claudeFlow = true; add.error = ""; renderAgents();
    try { await invoke("claude_sign_in", { name: named.name }); if (ACC.add === add) ACC.add = null; } catch (error) { add.error = String(error); }
    ACC.claudeFlow = false; redrawAgents();
  },
  async removeNamed(agent, name) {
    ACC.busy = `remove:${agent}/${name}`; renderAgents();
    ACC.namedError = null;
    try { await invoke("revoke_agent_token", { agentSlug: agent, name }); }
    catch (error) { ACC.namedError = { agent, text: String(error) }; }
    ACC.busy = ""; redrawAgents();
  },
  edit(agent) { agentEditVersion++; ACC.edit = agent; ACC.add = null; ACC.error = ""; renderAgents(); },
  cancel() { agentEditVersion++; ACC.edit = null; ACC.error = ""; renderAgents(); },
  async save(secret, clear) {
    if (ACC.busy) return;
    const draft = tokenDraft(ACC.edit, secret);
    if (!draft.agent) { ACC.error = draft.error; renderAgents(); return; }
    const version = agentEditVersion;
    ACC.busy = draft.agent; ACC.error = ""; renderAgents();
    try { await invoke("save_agent_token", { agentSlug: draft.agent, secret: draft.secret }); if (version === agentEditVersion) ACC.edit = null; }
    catch (error) { if (version === agentEditVersion) ACC.error = String(error); }
    ACC.busy = "";
    if (version === agentEditVersion) clear();
    redrawAgents();
  },
  async revoke(agent) {
    ACC.busy = `revoke:${agent}`; renderAgents();
    try { await invoke("revoke_agent_token", { agentSlug: agent }); }
    catch (error) { if (agent === "claude") ACC.claudeError = String(error); else ACC.error = String(error); }
    ACC.busy = ""; redrawAgents();
  },
  async claudeSign() {
    if (ACC.claudeFlow) return;
    agentEditVersion++; ACC.claudeFlow = true; ACC.claudeError = ""; ACC.edit = null; ACC.add = null; renderAgents();
    try { await invoke("claude_sign_in"); } catch (error) { ACC.claudeError = String(error); }
    ACC.claudeFlow = false; redrawAgents();
  },
  claudeCancel() { invoke("cancel_claude_sign_in").catch(console.warn); },
  machineSign(tool, machine) {
    if (!machine.online) return;

    openSignIn(machine, tool);
  },
  async codexSign(id) {
    if (ACC.codex[id]?.starting || ACC.codex[id]?.task !== undefined) return;
    ACC.codex[id] = { starting: true }; redrawAgents();
    try {
      const flow = await invoke<CodexLogin>("codex_login_start", { machineId: id });
      ACC.codex[id] = { ...flow, copied: false }; redrawAgents(); followCodex();
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
  open(url) { invoke("open_url", { url }).catch(console.warn); },
};
