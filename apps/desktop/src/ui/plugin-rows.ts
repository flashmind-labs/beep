// What the desktop says about its plugins, read and typed. The skin loader in this folder and
// the Plugins page in settings both read them.

export type PluginPermission = "ui.add" | "sessions.read" | "tools" | "style";

/// Where a plugin came from.
export type PluginSource =
  | { kind: "local" }
  | { kind: "dev" }
  | { kind: "builtin" }
  | { kind: "git"; url: string; ref: string | null; subfolder: string | null; commit: string };

/// Where a plugin stands, as the host reports it.
export type PluginRun =
  | { state: "waiting" }
  | { state: "running" }
  | { state: "paused" }
  | { state: "disabled" }
  | { state: "stopped"; reason: string }
  | { state: "needs-approval"; missing: PluginPermission[] };

/// One plugin of the list (`plugins_status`).
export interface PluginRow {
  id: string;
  name: string;
  version: string;
  description: string;
  permissions: PluginPermission[];
  approved: PluginPermission[];
  source: PluginSource;
  enabled: boolean;
  run: PluginRun;
  /// The style tokens its manifest sets, which make it a skin.
  style: ReadonlyMap<string, string>;
}

/// A skin a plugin offers in Appearance.
export interface PluginSkin {
  /// The plugin's id.
  id: string;
  name: string;
  /// A color that stands for it in the picker.
  swatch: string;
  tokens: ReadonlyMap<string, string>;
}

export const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
export const isText = (value: unknown): value is string => typeof value === "string";
const isFlag = (value: unknown): value is boolean => typeof value === "boolean";
export const textOrNull = (value: unknown): string | null | undefined => value === null ? null : isText(value) ? value : undefined;

function permission(value: unknown): PluginPermission | null {
  return value === "ui.add" || value === "sessions.read" || value === "tools" || value === "style" ? value : null;
}

export function permissions(value: unknown): PluginPermission[] | null {
  if (!Array.isArray(value)) return null;
  const found: PluginPermission[] = [];
  for (const item of value) {
    const known = permission(item);
    if (known === null) return null;
    found.push(known);
  }
  return found;
}

export function parseSource(raw: unknown): PluginSource | null {
  if (!isRecord(raw)) return null;
  if (raw.kind === "local") return { kind: "local" };
  if (raw.kind === "dev") return { kind: "dev" };
  if (raw.kind === "builtin") return { kind: "builtin" };
  if (raw.kind !== "git") return null;
  const ref = textOrNull(raw.ref ?? null), subfolder = textOrNull(raw.subfolder ?? null);
  if (!isText(raw.url) || !isText(raw.commit) || ref === undefined || subfolder === undefined) return null;
  return { kind: "git", url: raw.url, ref, subfolder, commit: raw.commit };
}

function parseRun(raw: Record<string, unknown>): PluginRun | null {
  switch (raw.state) {
    case "waiting": return { state: "waiting" };
    case "running": return { state: "running" };
    case "paused": return { state: "paused" };
    case "disabled": return { state: "disabled" };
    case "stopped": return isText(raw.reason) ? { state: "stopped", reason: raw.reason } : null;
    case "needs-approval": {
      const missing = permissions(raw.missing);
      return missing === null ? null : { state: "needs-approval", missing };
    }
    default: return null;
  }
}

function parseRow(raw: unknown): PluginRow | null {
  if (!isRecord(raw)) return null;
  const asked = permissions(raw.permissions), approved = permissions(raw.approved), source = parseSource(raw.source), run = parseRun(raw);
  if (!isText(raw.id) || !isText(raw.name) || !isText(raw.version) || !isText(raw.description) || !isFlag(raw.enabled)) return null;
  if (asked === null || approved === null || source === null || run === null) return null;
  const style = parseStyle(raw.style ?? {});
  if (style === null) return null;
  return { id: raw.id, name: raw.name, version: raw.version, description: raw.description, permissions: asked, approved, source, enabled: raw.enabled, run, style };
}

function parseStyle(raw: unknown): Map<string, string> | null {
  if (!isRecord(raw)) return null;
  const tokens = new Map<string, string>();
  for (const [token, value] of Object.entries(raw)) {
    if (!token.startsWith("--") || !isText(value)) return null;
    tokens.set(token, value);
  }
  return tokens;
}

/// The skins the plugins offer: those that set tokens, are on and may restyle the app.
export function pluginSkins(rows: readonly PluginRow[]): PluginSkin[] {
  return rows
    .filter(row => row.style.size > 0 && row.enabled && row.approved.includes("style") && row.run.state !== "disabled" && row.run.state !== "needs-approval")
    .map(row => ({ id: row.id, name: row.name, swatch: row.style.get("--surface") ?? row.style.get("--ink") ?? row.style.get("--accent") ?? "var(--surface)", tokens: row.style }));
}

/// The list `plugins_status` answers with; null when any row is not what the desktop promised.
export function parseRows(raw: unknown): PluginRow[] | null {
  if (!Array.isArray(raw)) return null;
  const rows: PluginRow[] = [];
  for (const item of raw) {
    const row = parseRow(item);
    if (row === null) return null;
    rows.push(row);
  }
  return rows;
}
