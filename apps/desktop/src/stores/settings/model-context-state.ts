import type { Compact } from "./providers-state.ts";
export interface ModelCompact { model: string; compact: NonNullable<Compact> }
export interface ModelCompactDraft { model: string; threshold: string }
export function modelDraft(rules: ModelCompact[]): ModelCompactDraft[] {
  return rules.map(({ model, compact }) => ({ model, threshold: !compact.enabled ? "off" : compact.tokens != null ? String(compact.tokens) : `${compact.percent ?? 80}%` }));
}
function parse(text: string): NonNullable<Compact> | null {
  const t = text.trim().toLowerCase();
  if (t === "off") return { enabled: false, percent: null, tokens: null };
  if (!/^\d+\s*%?$/.test(t)) return null;
  const n = Number(t.replace(/\s*%$/, ""));
  if (!Number.isSafeInteger(n) || n < 1 || (t.endsWith("%") && n > 100)) return null;
  return { enabled: true, percent: n <= 100 ? n : null, tokens: n > 100 ? n : null };
}
export function modelRules(draft: ModelCompactDraft[]): ModelCompact[] {
  return draft.flatMap(r => { const compact = parse(r.threshold); return compact ? [{ model: r.model.trim(), compact }] : []; });
}
export function modelProblem(draft: ModelCompactDraft[]) {
  const active = draft.filter(r => r.threshold.trim()), seen = new Set<string>();
  if (active.length > 64) return "At most 64 model overrides.";

  for (const r of active) {
    const id = r.model.trim();
    if (!id || id.length > 200 || /[\x00-\x1f\x7f]/.test(id)) return "Give each override a model id.";
    if (seen.has(id)) return `${id} is set twice.`;
    seen.add(id);
    if (!parse(r.threshold)) return `Use 1 to 100%, a token count, or off for ${id}.`;
  }

  return "";
}
