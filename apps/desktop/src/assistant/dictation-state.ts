// The Dictation section's logic: which provider is in use, the model it shows, and the search over
// OpenRouter's transcription models. Pure; dictation.ts draws it.

/// Who transcribes, as Rust's `Provider` names it.
export type DictationProvider = "openrouter" | "openai" | "mistral";

/// A provider with a saved key, and the model used until another is typed.
export interface Offered {
  provider: DictationProvider;
  name: string;
  default_model: string;
}

/// What `dictation_get` answers.
export interface DictationView {
  provider: DictationProvider | null;
  /// Empty means the provider's default.
  model: string;
  offered: Offered[];
}

/// A transcription model OpenRouter lists.
export interface ModelChoice {
  id: string;
  name: string;
}

/// The most models listed at once; the search narrows the rest.
export const MODEL_ROWS = 40;

/// The picked provider while its key is still saved, else off.
export function active(view: DictationView): Offered | null {
  return view.offered.find(o => o.provider === view.provider) ?? null;
}

/// The model dictation uses: the typed one, or the provider's default.
export function shownModel(view: DictationView): string {
  const on = active(view);
  if (!on) return "";
  return view.model.trim() || on.default_model;
}

/// The models whose name or id holds every word of `query`, at most `MODEL_ROWS`, and how many
/// more matched.
export function filterModels(models: readonly ModelChoice[], query: string): { rows: ModelChoice[]; more: number } {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const found = models.filter(m => {
    const text = `${m.name} ${m.id}`.toLowerCase();
    return words.every(w => text.includes(w));
  });
  return { rows: found.slice(0, MODEL_ROWS), more: Math.max(0, found.length - MODEL_ROWS) };
}

export const NO_KEYS = "Save an OpenRouter, OpenAI or Mistral key above to turn it on";
export const DICTATION_NOTE = "Hold Option and speak: the words are typed where your cursor is. Nothing is sent to a chat.";
