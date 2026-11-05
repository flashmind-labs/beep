// The words and sums of the local model picker, as pure functions so they can be tested without
// Tauri. modelsearch.ts draws it.

/// What a model can do, with the labels and in the order of the flashmind CLI's CATALOG_FEATURES
/// (assistant `Feature`).
export type Feature = "chat" | "tools" | "reasoning" | "vision" | "documents" | "audio-input" | "video-input"
  | "image" | "video" | "image-to-video" | "speech" | "transcribe" | "image-edit" | "style-reference";
export const FEATURES: Feature[] = ["chat", "tools", "reasoning", "vision", "documents", "audio-input", "video-input",
  "image", "video", "image-to-video", "speech", "transcribe", "image-edit", "style-reference"];

/// A model of the catalog (assistant `ModelInfo`).
export interface CatalogModel {
  id: string;
  name: string;
  /// Seconds since the epoch.
  created: number | null;
  context_length: number | null;
  features: Feature[];
  tools: boolean;
  images: boolean;
  /// USD per million tokens, when listed.
  input_per_m: number | null;
  output_per_m: number | null;
}

/// An upstream provider that serves a model (assistant `Provider`).
export interface ProviderOption {
  tag: string;
  name: string;
}

/// The provider saved with the model it was picked for (assistant `ProviderPref`).
export interface ProviderPref {
  model: string;
  tag: string;
}

/// How well `words` match: 0 when the name or id starts with the first word, else 1.
function rank(model: CatalogModel, words: string[]) {
  const first = words[0];
  if (!first) return 0;
  return model.name.toLowerCase().startsWith(first) || model.id.toLowerCase().startsWith(first) ? 0 : 1;
}

/// The models whose name and id hold every typed word, case aside, and every active feature:
/// newest first, then best match, then by id.
export function filterModels(models: CatalogModel[], query: string, active: Feature[]): CatalogModel[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hits = models.filter(m => {
    const text = `${m.name} ${m.id}`.toLowerCase();
    return words.every(w => text.includes(w)) && active.every(f => m.features.includes(f));
  });
  return hits
    .map(m => ({ m, r: rank(m, words) }))
    .sort((a, b) => (b.m.created ?? -1) - (a.m.created ?? -1) || a.r - b.r || (a.m.id < b.m.id ? -1 : a.m.id > b.m.id ? 1 : 0))
    .map(x => x.m);
}

/// The active features with `feature` turned on or off. Chat stays on: the list holds chat models.
export function toggleFeature(active: Feature[], feature: Feature): Feature[] {
  if (feature === "chat") return active;
  return active.includes(feature) ? active.filter(f => f !== feature) : [...active, feature];
}

/// The row picked after `key` among `n` rows, wrapping at both ends.
export function stepSel(sel: number, n: number, key: string) {
  if (!n) return 0;
  if (key === "ArrowDown") return (sel + 1) % n;
  if (key === "ArrowUp") return (sel + n - 1) % n;
  return sel;
}

/// A context length as `200k` or `1M`.
export function contextLabel(tokens: number | null) {
  if (!tokens) return "";
  if (tokens >= 1_000_000) return `${Math.round(tokens / 100_000) / 10}M`;
  return `${Math.round(tokens / 1000)}k`;
}

const usd = (n: number) => "$" + (Math.round(n * 100) / 100);

/// A row's second line: context and price.
export function detailLine(m: CatalogModel) {
  const price = m.input_per_m != null && m.output_per_m != null
    ? `${usd(m.input_per_m)} in, ${usd(m.output_per_m)} out per M tokens`
    : "No price listed";
  const context = contextLabel(m.context_length);
  return context ? `${context} context, ${price}` : price;
}

/// The provider tag preferred for `model`: only one picked for it counts.
export function providerFor(pref: ProviderPref | null, model: string) {
  return pref && pref.model === model ? pref.tag : null;
}
