// Pure state and parsing helpers for editor LSP go-to-definition and synced
// Vim mode preferences. No DOM or Tauri calls here, so unit tests run directly.

export interface LspLocation {
  path: string;
  inside_root: boolean;
  line: number;
  column: number;
}

export interface LspDefinitionResult {
  locations: LspLocation[];
  note: string | null;
}

export interface EditorSettings {
  vim: boolean | null;
}

const IDENT_CHAR = /[A-Za-z0-9_$]/;

/// The identifier span `[from, to)` covering `column` in `line`, or null when
/// `column` is past the line end or on punctuation/whitespace.
export function wordSpanAt(line: string, column: number): { from: number; to: number } | null {
  if (column < 0 || column >= line.length || !IDENT_CHAR.test(line[column] ?? "")) return null;

  let from = column;
  while (from > 0 && IDENT_CHAR.test(line[from - 1] ?? "")) from -= 1;

  let to = column + 1;
  while (to < line.length && IDENT_CHAR.test(line[to] ?? "")) to += 1;

  return { from, to };
}

/// Short menu label for a definition target (`path:1-based-line`).
export function formatLocationLabel(location: LspLocation): string {
  return `${location.path}:${location.line + 1}`;
}

/// Reconciles localStorage (`"true"` | `"false"` | `null`) with mesh-synced
/// `EditorSettings.vim` (`boolean | null`). Mesh wins once saved; otherwise an
/// existing local preference is promoted to the mesh.
export function resolveVimSync(local: string | null, remote: boolean | null): {
  enabled: boolean;
  writeLocal: boolean;
  writeRemote: boolean;
} {
  if (remote !== null) {
    return { enabled: remote, writeLocal: local !== String(remote), writeRemote: false };
  }

  if (local === "true" || local === "false") {
    return { enabled: local === "true", writeLocal: false, writeRemote: true };
  }

  return { enabled: false, writeLocal: false, writeRemote: false };
}

/// Validates the Tauri IPC reply from `agent_lsp_definition`.
export function parseLspResult(raw: unknown): LspDefinitionResult {
  if (!raw || typeof raw !== "object") return { locations: [], note: null };

  const note = "note" in raw && typeof raw.note === "string" ? raw.note : null;
  const rawLocations = "locations" in raw && Array.isArray(raw.locations) ? raw.locations : [];
  const locations: LspLocation[] = [];

  for (const item of rawLocations) {
    if (!item || typeof item !== "object") continue;
    if (!("path" in item && typeof item.path === "string")) continue;
    if (!("line" in item && typeof item.line === "number")) continue;
    if (!("column" in item && typeof item.column === "number")) continue;

    locations.push({
      path: item.path,
      inside_root: "inside_root" in item && typeof item.inside_root === "boolean" ? item.inside_root : false,
      line: item.line,
      column: item.column,
    });
  }

  return { locations, note };
}

/// Validates the Tauri IPC reply from `editor_settings_get`.
export function parseEditorSettings(raw: unknown): EditorSettings {
  if (!raw || typeof raw !== "object") return { vim: null };
  const vim = "vim" in raw && typeof raw.vim === "boolean" ? raw.vim : null;
  return { vim };
}
