// The Assistant page's chats: what the commands give and the pure choices around them. Pure;
// chat.ts and page.ts draw it.
import type { KeyRow } from "./assistant-state.ts";

/// A chat in the list, as `assistant_chats` gives it.
export interface ChatSummary {
  id: string;
  title: string;
  model: string;
  updated_ms: number;
}

/// A model OpenRouter lists. `tools` is false when it takes no tool definitions.
export interface ModelInfo {
  id: string;
  name: string;
  tools: boolean;
}

/// `assistant_models`: the list and the model a new chat starts on.
export interface ModelList {
  models: ModelInfo[];
  default: string | null;
}

/// A tool call of a saved turn.
export interface StoredStep {
  call_id: string;
  name: string;
  label: string;
  ok: boolean;
}

/// A saved prompt and its reply; the answer is sanitized HTML.
export interface StoredTurn {
  question: string;
  reasoning: string;
  steps: StoredStep[];
  answer_html: string;
}

/// `assistant_open`.
export interface OpenChat {
  id: string;
  title: string;
  model: string;
  running: boolean;
  turns: StoredTurn[];
}

const readable = (rows: readonly KeyRow[], slug: string) => rows.some(r => r.provider.slug === slug && r.saved != null && !r.saved.unreadable);

/// Whether the chat shows: an OpenRouter key is saved and this Mac can open it. Firecrawl alone is
/// not enough.
export const canChat = (rows: readonly KeyRow[]) => readable(rows, "openrouter");

/// Whether the web tools are on: a Firecrawl key is saved and readable.
export const webOn = (rows: readonly KeyRow[]) => readable(rows, "firecrawl");

/// The most models the picker draws at once.
export const MODELS_SHOWN = 60;

/// The models whose name or id has every word of `query`, in the order given.
export function filterModels(models: readonly ModelInfo[], query: string): ModelInfo[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hit = (m: ModelInfo) => words.every(w => m.id.toLowerCase().includes(w) || m.name.toLowerCase().includes(w));
  return models.filter(hit).slice(0, MODELS_SHOWN);
}

/// The user's date as `2026-11-05`, for the system prompt.
export function localDate(now = new Date()) {
  const two = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())}`;
}

/// What the pill shows for model `id`: its listed name, else the id.
export function modelLabel(models: readonly ModelInfo[] | null, id: string) {
  if (!id) return "Choose a model";
  return models?.find(m => m.id === id)?.name ?? id;
}
