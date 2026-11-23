// Challenging a report: a fresh agent, in a throwaway copy of the work,
// tries to prove the report's claims wrong. What it finds comes back as
// review drafts on the claims it disputes, for the user to send or drop.
import { createStore } from "zustand";
import type { ReviewComment } from "./review-state.ts";

/// How sure a finding is: shown by running something, or read in the code.
export type Tag = "confirmed" | "plausible";

/// A claim the challenger broke, as its report puts it under Needs you.
export interface Finding {
  tag: Tag;
  /// The claim: `Headline`, `Changed 2`.
  place: string;
  /// The claim's words it disputes.
  quote: string;
  text: string;
  evidence: string;
}

/// A challenge of the report at `entry`, by the session `challenger`.
export interface Challenge {
  entry: number;
  /// How the report starts, so a sent finding names it.
  opening: string;
  challenger: { machine: string; agent: string; session: string };
  state: "running" | "done";
  /// How many findings it left as drafts, once done.
  findings: number;
}

/// The message that starts a challenger: the skill to follow, where the
/// work is, and the report.
export function challengePrompt(report: string, cwd: string) {
  return `Challenge the report below, following Beep's \`challenge\` skill: try to prove its claims wrong, in a throwaway copy of the work. Another agent wrote it in a session that runs in \`${cwd}\`; its work is there or in one of that checkout's worktrees.\n\n<report>\n${report.trim()}\n</report>`;
}

/// The findings in a challenger's report: its Needs you items in the
/// challenge skill's shape. Other items are left out.
export function parseFindings(text: string): Finding[] {
  const section = /^##\s+Needs you\s*$([\s\S]*?)(?=^##\s|(?![\s\S]))/m.exec(text)?.[1] ?? "";
  const items = section.split(/^\s*\d+\.\s+/m).slice(1);
  const out: Finding[] = [];

  for (const item of items) {
    const head = /^\*\*\[(confirmed|plausible)\]\s*(.+?)\*\*\s*$/m.exec(item);
    if (!head) continue;

    const lines = item.slice(head.index + head[0].length).split("\n").map(line => line.trim()).filter(Boolean);
    const quote = lines.filter(line => line.startsWith(">")).map(line => line.replace(/^>\s?/, "")).join(" ");
    const evidence = lines.find(line => /^evidence:/i.test(line))?.replace(/^evidence:\s*/i, "") ?? "";
    const words = lines.filter(line => !line.startsWith(">") && !/^evidence:/i.test(line)).join(" ");
    out.push({ tag: head[1] === "confirmed" ? "confirmed" : "plausible", place: head[2].trim(), quote: plain(quote), text: words, evidence });
  }

  return out;
}

/// A finding as a review draft on the report at `entry`, which starts `opening`.
export function findingDraft(finding: Finding, id: number, entry: number, opening: string): ReviewComment {
  const tag = finding.tag === "confirmed" ? "Confirmed" : "Plausible";
  const evidence = finding.evidence ? `\nEvidence: ${finding.evidence}` : "";
  return { id, entry, quote: finding.quote, place: finding.place, opening, ask: "finding", text: `${tag}: ${finding.text}${evidence}` };
}

/// `text` without Markdown's emphasis and code marks, as the reply shows it.
const plain = (text: string) => text.replace(/\*\*|__|`/g, "").trim();

// ----- Kept on this machine -----

const STORAGE = "beep.chatChallenges";

/// Every session's challenges, by `machine:session` as reviews are keyed.
export const challenges = createStore<{ by: Readonly<Record<string, readonly Challenge[]>> }>(() => ({ by: loadChallenges() }));

/// Changes the challenges of `key`, keeping them on this machine.
export function updateChallenges(key: string, change: (list: readonly Challenge[]) => readonly Challenge[]) {
  challenges.setState(state => ({ by: { ...state.by, [key]: change(state.by[key] ?? []) } }));
  try {
    localStorage.setItem(STORAGE, JSON.stringify(challenges.getState().by));
  } catch { /* kept for this launch only */ }
}

/// The last challenge of the report at `entry` in `key`'s session.
export const challengeOf = (by: Readonly<Record<string, readonly Challenge[]>>, key: string | null, entry: number) =>
  key === null ? null : [...by[key] ?? []].reverse().find(c => c.entry === entry) ?? null;

function loadChallenges(): Record<string, readonly Challenge[]> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(STORAGE) ?? "{}");
    if (typeof raw !== "object" || raw === null) return {};
    return Object.fromEntries(Object.entries(raw).map(([key, list]) => [key, Array.isArray(list) ? list.filter(isChallenge) : []]));
  } catch {
    return {};
  }
}

function isChallenge(value: unknown): value is Challenge {
  if (typeof value !== "object" || value === null) return false;
  if (!("entry" in value && typeof value.entry === "number" && "opening" in value && typeof value.opening === "string" && "findings" in value && typeof value.findings === "number")) return false;
  if (!("state" in value && (value.state === "running" || value.state === "done"))) return false;
  if (!("challenger" in value && typeof value.challenger === "object" && value.challenger !== null)) return false;

  const c = value.challenger;
  return "machine" in c && typeof c.machine === "string" && "agent" in c && typeof c.agent === "string" && "session" in c && typeof c.session === "string";
}
