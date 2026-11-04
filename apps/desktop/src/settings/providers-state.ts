// The Coding agents page's words and sums, as pure functions so they can be
// tested without Tauri. providers.ts draws the page.

/// How a harness signs in (SignInView).
interface SignInView {
  state: "unknown" | "signed_in" | "signed_out" | "beep_token";
  account: string | null;
  plan: string | null;
  last4: string | null;
}

/// The user's settings for one harness (SettingsView): env holds names only.
interface SettingsView {
  enabled: boolean;
  program: string;
  home: string;
  args: string[];
  env: string[];
}

/// One harness on a machine (HarnessView).
export interface HarnessView {
  agent: string;
  installed: boolean;
  runnable: boolean;
  version: string | null;
  latest: string | null;
  program: string | null;
  home_var: string | null;
  home_default: string | null;
  sign_in: SignInView;
  install: string | null;
  settings: SettingsView;
}

/// One machine's harnesses (HarnessesView).
export interface HarnessesView {
  machine_id: string;
  machine_name: string;
  /// Milliseconds since the Unix epoch; null when never checked.
  checked_at: number | null;
  harnesses: HarnessView[];
}

/// A row of the list on the left.
interface HarnessRow {
  agent: string;
  name: string;
  initials: string;
  version: string | null;
  update: boolean;
  status: string;
  /// The status is a problem, drawn in coral.
  bad: boolean;
  on: boolean;
  /// It has a switch: only an installed harness can be turned off.
  switchable: boolean;
}

/// An env var row as the form holds it: `saved` when the server has a value
/// for it, `value` what was typed (empty keeps the saved one).
interface EnvDraft {
  name: string;
  value: string;
  saved: boolean;
}

/// The runtime fields as the form holds them.
export interface HarnessDraft {
  program: string;
  home: string;
  args: string;
  env: EnvDraft[];
}

/// What `harness_settings_save` takes.
interface SettingsInput {
  enabled: boolean;
  program: string;
  home: string;
  args: string[];
  env: { name: string; value: string | null }[];
}

const NAMES: Record<string, [string, string]> = {
  claude: ["Claude Code", "CC"],
  codex: ["Codex", "CX"],
  pi: ["pi", "PI"],
  gemini: ["Gemini CLI", "GE"],
  flsh: ["flsh", "FL"],
  opencode: ["OpenCode", "OC"],
};

/// What each harness is called on screen.
export function harnessName(agent: string) { return NAMES[agent]?.[0] ?? agent; }

/// Its two letters in the avatar.
export function initials(agent: string) { return NAMES[agent]?.[1] ?? agent.slice(0, 2).toUpperCase(); }

/// Whether version `latest` is newer than `current`, number by number.
export function isNewer(latest: string, current: string) {
  const nums = (v: string) => v.split(/[-+]/)[0].split(".").map(n => Number.parseInt(n, 10) || 0);
  const a = nums(latest), b = nums(current);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0, y = b[i] ?? 0;
    if (x !== y) return x > y;
  }
  return false;
}

/// Whether a newer version than the one installed is out.
export function hasUpdate(h: HarnessView) {
  return h.installed && !!h.version && !!h.latest && isNewer(h.latest, h.version);
}

/// " · Max plan": the plan as the agent gives it, capitalised, or "".
export const planOf = (s: SignInView) => (s.plan ? ` · ${s.plan[0].toUpperCase()}${s.plan.slice(1)} plan` : "");

/// The line under a harness's name: its sign-in, or why it cannot run.
export function statusLine(h: HarnessView): { text: string; bad: boolean } {
  if (!h.installed) return { text: "Not installed", bad: false };
  if (!h.settings.enabled) return { text: "Turned off: Auto and the start form skip it", bad: false };
  const s = h.sign_in;
  if (s.state === "beep_token") return { text: `Signs in with Beep's token …${s.last4 ?? ""}`, bad: false };
  if (s.state === "signed_out") return { text: "Not signed in on this machine", bad: true };
  if (s.state === "signed_in") return { text: `Signed in${planOf(s)}`, bad: false };
  if (!h.runnable) return { text: "Installed; Beep cannot start it yet", bad: true };
  return { text: "Installed", bad: false };
}

/// The one status line atop a harness's own page: as the list says it, but
/// naming the account it is signed in with.
export function summaryLine(h: HarnessView): { text: string; bad: boolean } {
  const s = h.sign_in;
  if (h.installed && h.settings.enabled && s.state === "signed_in" && s.account) return { text: `Signed in as ${s.account}${planOf(s)}`, bad: false };
  return statusLine(h);
}

/// The list rows: every harness Beep knows, OpenCode only when the machine
/// has it.
export function listRows(view: HarnessesView): HarnessRow[] {
  return view.harnesses
    .filter(h => h.agent !== "opencode" || h.installed || h.runnable)
    .map(h => {
      const status = statusLine(h);
      return {
        agent: h.agent,
        name: harnessName(h.agent),
        initials: initials(h.agent),
        version: h.installed ? h.version : null,
        update: hasUpdate(h),
        status: status.text,
        bad: status.bad,
        on: h.installed && h.settings.enabled,
        switchable: h.installed,
      };
    });
}

/// "v2.1.312 · v2.1.320 out", "v0.166.0", or "" when not installed.
export function versionLine(h: HarnessView) {
  if (!h.installed || !h.version) return "";
  return hasUpdate(h) ? `v${h.version} · v${h.latest} out` : `v${h.version}`;
}

/// "Checked just now", "Checked 4 min ago", "Checked 2 h ago", or "Check
/// for updates" when the machine never looked.
export function checkedLabel(checkedAt: number | null | undefined, now: number) {
  if (!checkedAt) return "Check for updates";
  const min = Math.floor((now - checkedAt) / 60000);
  if (min < 1) return "Checked just now";
  if (min < 60) return `Checked ${min} min ago`;
  const h = Math.floor(min / 60);
  if (h < 48) return `Checked ${h} h ago`;
  return `Checked ${Math.floor(h / 24)} days ago`;
}

/// The harnesses Update all updates: installed, behind, and with a command.
export function updatable(view: HarnessesView | null | undefined) {
  return (view?.harnesses ?? []).filter(h => hasUpdate(h) && !!h.install).map(h => h.agent);
}

/// Splits launch arguments as a shell would for plain words and quotes:
/// `--a 'b c' "d"` gives `--a`, `b c`, `d`.
export function splitArgs(text: string) {
  const out: string[] = [];
  let cur = "", quote: string | null = null, any = false;
  for (const c of text) {
    if (quote) {
      if (c === quote) quote = null;
      else cur += c;
    } else if (c === "'" || c === '"') {
      quote = c;
      any = true;
    } else if (/\s/.test(c)) {
      if (cur || any) out.push(cur);
      cur = "";
      any = false;
    } else {
      cur += c;
    }
  }
  if (cur || any) out.push(cur);
  return out;
}

/// The arguments as one line the form shows, quoting those with spaces.
export function joinArgs(args: string[]) {
  return args.map(a => (a === "" || /[\s'"]/.test(a) ? (a.includes("'") ? `"${a}"` : `'${a}'`) : a)).join(" ");
}

/// The form's fields for `h`'s saved settings.
export function draftOf(h: HarnessView): HarnessDraft {
  return {
    program: h.settings.program,
    home: h.settings.home,
    args: joinArgs(h.settings.args),
    env: h.settings.env.map(name => ({ name, value: "", saved: true })),
  };
}

/// Whether the form differs from what is saved.
export function draftChanged(h: HarnessView, draft: HarnessDraft) {
  const saved = draftOf(h);
  if (draft.program.trim() !== saved.program || draft.home.trim() !== saved.home) return true;
  if (splitArgs(draft.args).join("\u0000") !== h.settings.args.join("\u0000")) return true;
  const named = draft.env.filter(v => v.name.trim());
  if (named.some(v => v.value !== "")) return true;
  return named.map(v => v.name.trim()).join(",") !== h.settings.env.join(",");
}

/// What Save sends: the form's fields, an env var's value only when typed
/// (null keeps the saved one), empty rows left out.
export function settingsInput(enabled: boolean, draft: HarnessDraft): SettingsInput {
  return {
    enabled,
    program: draft.program.trim(),
    home: draft.home.trim(),
    args: splitArgs(draft.args),
    env: draft.env
      .filter(v => v.name.trim())
      .map(v => ({ name: v.name.trim(), value: v.value !== "" ? v.value : v.saved ? null : "" })),
  };
}

/// What turning the switch sends: the saved settings with `enabled` flipped.
export function toggledInput(h: HarnessView, enabled: boolean): SettingsInput {
  return {
    enabled,
    program: h.settings.program,
    home: h.settings.home,
    args: h.settings.args,
    env: h.settings.env.map(name => ({ name, value: null })),
  };
}

/// Why the env rows cannot be saved, or "" when they can.
export function envProblem(env: EnvDraft[]) {
  const seen = new Set<string>();
  for (const v of env) {
    const name = v.name.trim();
    if (!name) continue;
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) return `${name} is not a variable name.`;
    if (seen.has(name)) return `${name} is set twice.`;
    seen.add(name);
  }
  return "";
}

/// One machine's saved settings for a harness (MachineSettingsView).
export interface MachineSettings {
  machine_id: string;
  machine_name: string;
  settings: SettingsView;
}

/// A field "Save to all hosts" copies, and so can differ between machines.
export type SharedField = "program" | "home" | "args" | "env";

/// A field's value as compared across machines: env vars by their names,
/// in any order, since the values never come back.
function fieldKey(settings: SettingsView, field: SharedField) {
  if (field === "args") return JSON.stringify(settings.args);
  if (field === "env") return JSON.stringify([...settings.env].sort());
  return settings[field];
}

/// The other machines whose saved `field` is not what `machineId` has saved, by
/// name; empty when it is the same everywhere or the machine is not in `all`.
export function differsOn(all: readonly MachineSettings[], machineId: string, field: SharedField): string[] {
  const own = all.find(m => m.machine_id === machineId);
  if (!own) return [];
  const mine = fieldKey(own.settings, field);
  return all.filter(m => m.machine_id !== machineId && fieldKey(m.settings, field) !== mine).map(m => m.machine_name);
}

/// "differs on 2 machines", or "" when it does not.
export function differsLabel(names: readonly string[]) {
  return names.length ? `differs on ${names.length} machine${names.length === 1 ? "" : "s"}` : "";
}

/// "Saved to 3 machines".
export function savedToLabel(n: number) {
  return `Saved to ${n} machine${n === 1 ? "" : "s"}`;
}
