// The Plugins page's words, checks and install flow, as pure functions so they
// can be tested without Tauri. plugins-settings.ts runs them and
// plugins-settings-view.tsx draws them.

import { isRecord, isText, parseSource, permissions, textOrNull, type PluginPermission, type PluginRow, type PluginRun, type PluginSource } from "../ui/plugin-rows.ts";

type PreviewKind = "new" | "update" | "allow";

/// What installing, updating or allowing a plugin would do.
export interface PluginPreview {
  id: string;
  name: string;
  version: string;
  description: string;
  permissions: PluginPermission[];
  source: PluginSource;
  kind: PreviewKind;
  previousVersion: string | null;
  added: PluginPermission[];
  removed: PluginPermission[];
}

/// A plugin fetched into a staging folder, waiting for the user's answer.
export interface StagedPlugin {
  stage: string;
  preview: PluginPreview;
}

export type UpdateCheck = { state: "current" } | { state: "available"; commit: string } | { state: "pinned" };

/// What a plugins command says when it fails: a kind to branch on and a message to show.
export interface PluginFailure {
  kind: string;
  message: string;
}

// ---- Words ----

const LABELS: Record<PluginPermission, { label: string; meaning: string }> = {
  "ui.add": { label: "Add to the interface", meaning: "Show text, buttons and images in the message box, session rows and session header." },
  "sessions.read": { label: "Read your sessions", meaning: "See your coding agent sessions and what each one is doing." },
  tools: { label: "Give tools to your agents", meaning: "Coding agents can call the tools this plugin lists." },
  style: { label: "Restyle the app", meaning: "Change the colours, corners, fonts, glass and paintings of every window." },
};

export const permissionLabel = (permission: PluginPermission) => LABELS[permission].label;
export const permissionMeaning = (permission: PluginPermission) => LABELS[permission].meaning;

/// "github.com/owner/repo at 4f0915a", "Local folder" or "Dev folder".
export function sourceLabel(source: PluginSource): string {
  switch (source.kind) {
    case "local": return "Local folder";
    case "dev": return "Dev folder";
    case "builtin": return "Built in";
    case "git": {
      const where = source.subfolder === null ? repositoryName(source.url) : `${repositoryName(source.url)}, ${source.subfolder}`;
      return `${where} at ${source.commit.slice(0, 7)}`;
    }
  }
}

/// Where a consent sheet says the plugin comes from.
export function sourceSentence(source: PluginSource): string {
  switch (source.kind) {
    case "local": return "From a folder on this computer";
    case "dev": return "From the development plugins folder";
    case "builtin": return "Ships with Beep";
    case "git": {
      const where = source.subfolder === null ? repositoryName(source.url) : `${repositoryName(source.url)}, ${source.subfolder}`;
      return `From ${where}, commit ${source.commit.slice(0, 7)}`;
    }
  }
}

/// A repository address without its scheme, user and `.git`: `github.com/owner/repo`.
export function repositoryName(url: string): string {
  const scheme = /^[a-z]+:\/\//.test(url);
  const bare = url.replace(/^[a-z]+:\/\//, "").replace(/^[^@/]+@/, "").replace(/\.git$/, "");
  // `git@host:owner/repo` writes its path after a colon; `host:8443/path` is a port.
  return scheme ? bare : bare.replace(/^([^/:]+):/, "$1/");
}

export type StateTone = "ok" | "off" | "warn" | "err";

/// The state line of a row. A skin waits for its slot only if it also has code.
export function runLine(run: PluginRun, skin: boolean): { text: string; tone: StateTone } {
  switch (run.state) {
    case "running": return { text: "On", tone: "ok" };
    case "waiting": return skin ? { text: "Pick it in Appearance", tone: "ok" } : { text: "Waiting for its slot", tone: "off" };
    case "paused": return { text: "Paused", tone: "off" };
    case "disabled": return { text: "Off", tone: "off" };
    case "needs-approval": return { text: "Needs approval", tone: "warn" };
    case "stopped": return { text: `Stopped: ${run.reason}`, tone: "err" };
  }
}

/// What a row lets the user do.
export interface RowOffers {
  /// The switch shows on.
  on: boolean;
  /// The switch cannot be used: the plugin must be allowed first.
  switchLocked: boolean;
  update: boolean;
  allow: boolean;
  remove: boolean;
}

export function rowOffers(row: PluginRow): RowOffers {
  const needsApproval = row.run.state === "needs-approval";
  return { on: row.enabled && !needsApproval, switchLocked: needsApproval, update: row.source.kind === "git", allow: needsApproval, remove: row.source.kind !== "builtin" };
}

export function consentTitle(preview: PluginPreview): string {
  switch (preview.kind) {
    case "new": return `Install ${preview.name}?`;
    case "update": return `Update ${preview.name}?`;
    case "allow": return `Allow ${preview.name}?`;
  }
}

export function consentAction(preview: PluginPreview): string {
  switch (preview.kind) {
    case "new": return "Allow and install";
    case "update": return "Allow and update";
    case "allow": return "Allow";
  }
}

/// "Composer cat 0.1.0 to 0.2.0" for an update, "Composer cat 0.2.0" otherwise.
export function consentVersions(preview: PluginPreview): string {
  const from = preview.previousVersion;
  return preview.kind === "update" && from !== null && from !== preview.version ? `${preview.name} ${from} to ${preview.version}` : `${preview.name} ${preview.version}`;
}

/// A permission of a preview is new when it is not allowed yet and the sheet
/// is not just listing everything for a first install.
export function isNewPermission(preview: PluginPreview, permission: PluginPermission): boolean {
  return preview.kind !== "new" && preview.added.includes(permission);
}

export function updateLine(check: UpdateState): string {
  switch (check.kind) {
    case "checking": return "Checking for an update…";
    case "current": return "Up to date";
    case "pinned": return "Installed at a fixed commit, so there is nothing newer to find";
    case "failed": return check.message;
  }
}

export type UpdateState =
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "pinned" }
  | { kind: "failed"; message: string };

// ---- The install flow ----

/// What the install actions are doing.
export type Flow =
  | { kind: "idle"; failure: PluginFailure | null }
  | { kind: "git"; repository: string; reference: string; folder: string; failure: PluginFailure | null }
  | { kind: "fetching"; repository: string; reference: string; folder: string }
  /// Reading a folder or an update, which has no form to stay in.
  | { kind: "working" }
  | { kind: "consent"; consent: Consent; busy: boolean; failure: PluginFailure | null };

/// What the consent sheet agrees to: a staged plugin, or an installed one that waits for approval.
type Consent = { source: "staged"; stage: string; preview: PluginPreview } | { source: "installed"; preview: PluginPreview };

export type GitField = "repository" | "reference" | "folder";

export type FlowEvent =
  | { type: "open-git" }
  | { type: "edit-git"; field: GitField; value: string }
  | { type: "submit-git" }
  | { type: "work" }
  | { type: "staged"; staged: StagedPlugin }
  | { type: "reviewed"; preview: PluginPreview }
  | { type: "failed"; failure: PluginFailure }
  | { type: "confirming" }
  | { type: "closed" };

export const IDLE: Flow = { kind: "idle", failure: null };

const NEEDS_REPOSITORY: PluginFailure = { kind: "invalid", message: "Enter a repository, such as owner/repo." };

export function flowStep(flow: Flow, event: FlowEvent): Flow {
  switch (event.type) {
    case "open-git": return { kind: "git", repository: "", reference: "", folder: "", failure: null };
    case "edit-git": return flow.kind === "git" ? { ...flow, [event.field]: event.value, failure: null } : flow;
    case "submit-git": {
      if (flow.kind !== "git") return flow;
      if (flow.repository.trim() === "") return { ...flow, failure: NEEDS_REPOSITORY };
      return { kind: "fetching", repository: flow.repository, reference: flow.reference, folder: flow.folder };
    }
    case "work": return flow.kind === "idle" ? { kind: "working" } : flow;
    case "staged": return { kind: "consent", consent: { source: "staged", stage: event.staged.stage, preview: event.staged.preview }, busy: false, failure: null };
    case "reviewed": return { kind: "consent", consent: { source: "installed", preview: event.preview }, busy: false, failure: null };
    case "failed": {
      if (flow.kind === "fetching") return { kind: "git", repository: flow.repository, reference: flow.reference, folder: flow.folder, failure: event.failure };
      if (flow.kind === "consent") return { ...flow, busy: false, failure: event.failure };
      return { kind: "idle", failure: event.failure };
    }
    case "confirming": return flow.kind === "consent" ? { ...flow, busy: true, failure: null } : flow;
    case "closed": return IDLE;
  }
}

/// The stage a flow holds that must be thrown away when it ends without an install.
export function openStage(flow: Flow): string | null {
  return flow.kind === "consent" && flow.consent.source === "staged" ? flow.consent.stage : null;
}

// ---- Reading what the desktop sends ----

export function parsePreview(raw: unknown): PluginPreview | null {
  if (!isRecord(raw)) return null;
  const asked = permissions(raw.permissions), added = permissions(raw.added), removed = permissions(raw.removed), source = parseSource(raw.source);
  const previous = textOrNull(raw.previousVersion ?? null);
  const kind = raw.kind === "new" || raw.kind === "update" || raw.kind === "allow" ? raw.kind : null;
  if (!isText(raw.id) || !isText(raw.name) || !isText(raw.version) || !isText(raw.description) || kind === null || previous === undefined) return null;
  if (asked === null || added === null || removed === null || source === null) return null;
  return { id: raw.id, name: raw.name, version: raw.version, description: raw.description, permissions: asked, source, kind, previousVersion: previous, added, removed };
}

export function parseStaged(raw: unknown): StagedPlugin | null {
  if (!isRecord(raw) || !isText(raw.stage)) return null;
  const preview = parsePreview(raw.preview);
  return preview === null ? null : { stage: raw.stage, preview };
}

export function parseUpdateCheck(raw: unknown): UpdateCheck | null {
  if (!isRecord(raw)) return null;
  if (raw.state === "current" || raw.state === "pinned") return { state: raw.state };
  if (raw.state === "available" && isText(raw.commit)) return { state: "available", commit: raw.commit };
  return null;
}

/// A failure as the desktop's typed error sends it (`{ kind, message }`); anything
/// else becomes a failure that carries its text, so nothing shows as blank.
export function parseFailure(raw: unknown): PluginFailure {
  if (isRecord(raw) && isText(raw.kind) && isText(raw.message)) return { kind: raw.kind, message: raw.message };
  const text = raw instanceof Error ? raw.message : isText(raw) ? raw : "";
  return { kind: "unknown", message: text === "" ? "Something went wrong. Try again." : text };
}

/// What the page says when the desktop's answer is not what it expects.
export const UNEXPECTED: PluginFailure = { kind: "unexpected", message: "Beep sent an answer this version cannot read. Restart Beep and try again." };
