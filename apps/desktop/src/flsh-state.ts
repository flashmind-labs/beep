// flsh's own settings on the Coding agents page: OpenRouter routing and
// auto-compact, the same on every machine. Pure, so the tests can drive
// it; flsh.ts draws it.

export const QUANTIZATIONS = ["int4", "int8", "fp4", "fp6", "fp8", "fp16", "bf16", "fp32", "unknown"] as const;
export const SORTS = ["price", "throughput", "latency"] as const;
export type Sort = (typeof SORTS)[number];

/// The bounds the server holds a compaction threshold to.
export const MIN_TOKENS = 1_000;
export const MAX_TOKENS = 10_000_000;

export interface AutoCompact { enabled: boolean; tokens: number | null }
export interface ModelRule { model: string; enabled: boolean; tokens: number | null }

/// What `flsh_settings_get` gives and `flsh_settings_save` takes. Null or
/// empty is flsh's own default.
export interface FlshSettings {
  zdr: boolean | null;
  quantizations: string[];
  sort: Sort | null;
  allow_fallbacks: boolean | null;
  autocompact: AutoCompact | null;
  models: ModelRule[];
}

/// The page's edit: thresholds as typed, so a half-typed number stays.
export interface FlshDraft {
  zdr: boolean | null;
  quantizations: string[];
  sort: Sort | null;
  allow_fallbacks: boolean | null;
  compact_on: boolean;
  compact_tokens: string;
  models: { model: string; enabled: boolean; tokens: string }[];
}

export const EMPTY: FlshSettings = { zdr: null, quantizations: [], sort: null, allow_fallbacks: null, autocompact: null, models: [] };

export function flshDraft(s: FlshSettings): FlshDraft {
  return {
    zdr: s.zdr,
    quantizations: QUANTIZATIONS.filter(q => s.quantizations.includes(q)),
    sort: s.sort,
    allow_fallbacks: s.allow_fallbacks,
    compact_on: s.autocompact?.enabled ?? true,
    compact_tokens: s.autocompact?.tokens != null ? String(s.autocompact.tokens) : "",
    models: s.models.map(m => ({ model: m.model, enabled: m.enabled, tokens: m.tokens != null ? String(m.tokens) : "" })),
  };
}

/// "300000", "300k" or "300,000" as a number; null when empty, NaN when
/// it is not a count.
export function parseTokens(text: string): number | null {
  const t = text.trim().toLowerCase().replace(/[_,\s]/g, "");
  if (!t) return null;
  const m = /^(\d+)(k?)$/.exec(t);
  if (!m) return NaN;
  return Number(m[1]) * (m[2] ? 1000 : 1);
}

const tokensOk = (n: number | null) => n === null || (Number.isInteger(n) && n >= MIN_TOKENS && n <= MAX_TOKENS);

/// Why the draft cannot be saved; "" when it can.
export function flshProblem(d: FlshDraft): string {
  if (!tokensOk(parseTokens(d.compact_tokens))) return `A threshold is a token count from ${MIN_TOKENS.toLocaleString("en")} to ${MAX_TOKENS.toLocaleString("en")}.`;
  const seen = new Set<string>();
  for (const m of d.models) {
    const name = m.model.trim();
    if (!/^[^:\s,]+:\S/.test(name)) return `Name each model provider:model, such as openrouter:vendor/model.`;
    if (seen.has(name.toLowerCase())) return `${name} is listed twice.`;
    seen.add(name.toLowerCase());
    if (!tokensOk(parseTokens(m.tokens))) return `The threshold for ${name} is a token count from ${MIN_TOKENS.toLocaleString("en")} to ${MAX_TOKENS.toLocaleString("en")}.`;
  }
  return "";
}

/// What the draft saves. The global rule is left to flsh while it is on
/// with no threshold, which is flsh's default.
export function flshInput(d: FlshDraft): FlshSettings {
  const tokens = parseTokens(d.compact_tokens);
  return {
    zdr: d.zdr,
    quantizations: QUANTIZATIONS.filter(q => d.quantizations.includes(q)),
    sort: d.sort,
    allow_fallbacks: d.allow_fallbacks,
    autocompact: d.compact_on && tokens === null ? null : { enabled: d.compact_on, tokens },
    models: d.models.map(m => ({ model: m.model.trim(), enabled: m.enabled, tokens: parseTokens(m.tokens) })),
  };
}

export function flshChanged(saved: FlshSettings, d: FlshDraft) {
  return JSON.stringify(flshInput(flshDraft(saved))) !== JSON.stringify(flshInput(d));
}

export function toggleQuantization(d: FlshDraft, q: string): string[] {
  return d.quantizations.includes(q) ? d.quantizations.filter(x => x !== q) : QUANTIZATIONS.filter(x => x === q || d.quantizations.includes(x));
}
