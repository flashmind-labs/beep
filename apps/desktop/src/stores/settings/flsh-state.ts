// flsh's own settings on the Coding agents page: OpenRouter routing, which
// pi takes too, and auto-compact, for all models and per model, the same on
// every machine.

export const QUANTIZATIONS = ["int4", "int8", "fp4", "fp6", "fp8", "fp16", "bf16", "fp32", "unknown"] as const;
export const SORTS = ["price", "throughput", "latency"] as const;
export type Sort = (typeof SORTS)[number];

/// The bounds Beep holds a compaction threshold to.
const MIN_TOKENS = 1_000;
const MAX_TOKENS = 10_000_000;

/// The agents that take these settings: flsh all of them, pi the routing.
export type RoutingAgent = "flsh" | "pi";

interface AutoCompact { enabled: boolean; tokens: number | null }
/// `enabled` and `tokens` are unused while the rule inherits compaction.
/// `quantizations` null keeps the list for all models; empty allows any.
interface ModelRule { model: string; inherits_compaction: boolean; enabled: boolean; tokens: number | null; quantizations: string[] | null }
/// How one model compacts: as all models do, or by its own rule.
export type ModelCompact = "all" | "on" | "off";
export interface ModelDraft { model: string; compact: ModelCompact; tokens: string; quantizations: string[] | null }

/// What `flsh_settings_get` gives and `flsh_settings_save` takes. Null or
/// empty is flsh's own default.
export interface FlshSettings {
  zdr: boolean | null;
  quantizations: string[];
  sort: Sort | null;
  allow_fallbacks: boolean | null;
  autocompact: AutoCompact | null;
  models: ModelRule[];
  stream_idle_timeout_secs: number | null;
}

/// The page's edit: thresholds as typed, so a half-typed number stays.
export interface FlshDraft {
  zdr: boolean | null;
  quantizations: string[];
  sort: Sort | null;
  allow_fallbacks: boolean | null;
  compact_on: boolean;
  compact_tokens: string;
  idle_seconds: string;
  models: ModelDraft[];
}

export const NEW_MODEL: ModelDraft = { model: "", compact: "all", tokens: "", quantizations: null };

export const EMPTY: FlshSettings = { zdr: null, quantizations: [], sort: null, allow_fallbacks: null, autocompact: null, models: [], stream_idle_timeout_secs: null };

export function flshDraft(s: FlshSettings): FlshDraft {
  return {
    zdr: s.zdr,
    quantizations: ordered(s.quantizations),
    sort: s.sort,
    idle_seconds: s.stream_idle_timeout_secs?.toString() ?? "",
    allow_fallbacks: s.allow_fallbacks,
    compact_on: s.autocompact?.enabled ?? true,
    compact_tokens: s.autocompact?.tokens != null ? String(s.autocompact.tokens) : "",
    models: s.models.map(m => ({
      model: m.model,
      compact: m.inherits_compaction ? "all" : m.enabled ? "on" : "off",
      tokens: !m.inherits_compaction && m.tokens != null ? String(m.tokens) : "",
      quantizations: m.quantizations && ordered(m.quantizations),
    })),
  };
}

/// "300000", "300k" or "300,000" as a number; null when empty, NaN when
/// it is not a count.
function parseTokens(text: string): number | null {
  const t = text.trim().toLowerCase().replace(/[_,\s]/g, "");
  if (!t) return null;
  const m = /^(\d+)(k?)$/.exec(t);
  if (!m) return NaN;
  return Number(m[1]) * (m[2] ? 1000 : 1);
}

const tokensOk = (n: number | null) => n === null || (Number.isInteger(n) && n >= MIN_TOKENS && n <= MAX_TOKENS);

/// Why the draft cannot be saved; "" when it can.
export function flshProblem(d: FlshDraft): string {
  const seconds = d.idle_seconds.trim();
  if (seconds && (!/^\d+$/.test(seconds) || Number(seconds) < 1 || Number(seconds) > 0xffff_ffff)) return "Enter a positive whole number of seconds for the stream idle timeout (at most 4,294,967,295).";
  if (!tokensOk(parseTokens(d.compact_tokens))) return `A threshold is a token count from ${MIN_TOKENS.toLocaleString("en")} to ${MAX_TOKENS.toLocaleString("en")}.`;
  const seen = new Set<string>();

  for (const m of d.models) {
    const name = m.model.trim();
    if (!/^[^:\s,]+:\S/.test(name)) return `Name each model provider:model, such as openrouter:vendor/model.`;
    if (seen.has(name.toLowerCase())) return `${name} is listed twice.`;
    seen.add(name.toLowerCase());
    if (m.compact !== "all" && !tokensOk(parseTokens(m.tokens))) return `The threshold for ${name} is a token count from ${MIN_TOKENS.toLocaleString("en")} to ${MAX_TOKENS.toLocaleString("en")}.`;
    if (m.quantizations !== null && !/^openrouter:/i.test(name)) return `${name} is not an OpenRouter model, so it has no quantizations to allow.`;
  }

  return "";
}

/// What the draft saves. The global rule is left to flsh while it is on
/// with no threshold, which is flsh's default.
export function flshInput(d: FlshDraft): FlshSettings {
  const tokens = parseTokens(d.compact_tokens);
  return {
    zdr: d.zdr,
    quantizations: ordered(d.quantizations),
    sort: d.sort,
    stream_idle_timeout_secs: d.idle_seconds.trim() ? Number(d.idle_seconds) : null,
    allow_fallbacks: d.allow_fallbacks,
    autocompact: d.compact_on && tokens === null ? null : { enabled: d.compact_on, tokens },
    models: d.models.map(m => ({
      model: m.model.trim(),
      inherits_compaction: m.compact === "all",
      enabled: m.compact !== "off",
      tokens: m.compact === "all" ? null : parseTokens(m.tokens),
      quantizations: m.quantizations && ordered(m.quantizations),
    })),
  };
}

export function flshChanged(saved: FlshSettings, d: FlshDraft) {
  return JSON.stringify(flshInput(flshDraft(saved))) !== JSON.stringify(flshInput(d));
}

export function toggleQuantization(list: string[], q: string): string[] {
  return list.includes(q) ? list.filter(x => x !== q) : ordered([...list, q]);
}

/// `list` in the page's order.
function ordered(list: string[]) { return QUANTIZATIONS.filter(q => list.includes(q)); }
