// Challenging a report: a fresh agent, in a throwaway copy of the work,
// tries to prove the report's claims wrong. What it finds comes back as
// review drafts on the claims it disputes, for the user to send or drop.
import { createStore } from "zustand";
import type { ReviewComment, Tag } from "./review-state.ts";


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

/// Where a challenge is: its challenger at work or asking the user, done
/// with its report, or ended without one.
export type ChallengeState = "running" | "asking" | "done" | "failed";

/// A challenge of the report at `entry`, by the session `challenger`.
export interface Challenge {
  entry: number;
  /// How the report starts, so a sent finding names it.
  opening: string;
  challenger: { machine: string; agent: string; session: string };
  state: ChallengeState;
  /// How many findings it left as drafts, once done.
  findings: number;
  /// The claims it checked and could not break, by place (`Changed 2`), once done.
  held: readonly string[];
  /// Why it failed; "" otherwise.
  error: string;
}

/// What a challenger's session status says of its challenge: still at it,
/// asking the user, finished (its reply is to be read), or gone. `status` is
/// null when the session is not listed, which a start not yet named is not.
export function challengeStep(status: string | null, starting: boolean): "running" | "asking" | "finished" | "gone" {
  if (status === null) return starting ? "running" : "gone";
  if (status === "asking" || status === "waiting") return "asking";
  return status === "idle" || status === "ended" ? "finished" : "running";
}

/// How a finished challenger's last reply ends its challenge: whatever it
/// said is its answer, its findings read from it, none when it broke no
/// claim, and the claims that held. Without a reply it failed, with the
/// session's own error if any.
export function challengeResult(reply: string | null, error: string | null): { state: "done"; findings: Finding[]; held: string[] } | { state: "failed"; error: string } {
  if (reply?.trim()) return { state: "done", findings: parseFindings(reply), held: parseHeld(reply) };
  return { state: "failed", error: error?.trim() || "It stopped without answering." };
}

/// The message that starts a challenger in its copy of the work in `work`:
/// the skill to follow, and the report.
export function challengePrompt(report: string, work: string) {
  return `Challenge the report below, following Beep's \`challenge\` skill: try to prove its claims wrong. Another agent wrote it about its work in \`${work}\`; you run in a throwaway copy of that work.\n\n<report>\n${report.trim()}\n</report>`;
}

/// The findings in a challenger's report: its Needs you items tagged
/// `[confirmed]` or `[plausible]`, read leniently as serpis reads them. An
/// item is `1.`, `1)` or `- ` at the line's start; the tag may be anywhere
/// in it, in any case, bold or not; `Place: text` may follow on that line.
/// Then come `> quote` lines, the finding's words and `Evidence:`, whose
/// words run on to the item's end. Untagged items are left out.
export function parseFindings(text: string): Finding[] {
  const section = sectionOf(text, "Needs you");
  const out: Finding[] = [];
  // Where an item's lines go: its words, its evidence, or nowhere (an untagged item).
  let into: "words" | "evidence" | "none" = "none";

  for (const line of section.split("\n")) {
    const t = line.trim();
    const item = /^\S/.test(line) ? /^(?:\d+[.)]|-)\s+(.*)$/.exec(t) : null;

    if (item) {
      const head = item[1].replace(/\*\*/g, "");
      const tag = /\[(confirmed|plausible)\]/i.exec(head);
      if (!tag) { into = "none"; continue; }

      const after = head.slice(tag.index + tag[0].length).trim(), colon = after.indexOf(":");
      const [place, words] = colon < 0 ? [after, ""] : [after.slice(0, colon).trim(), after.slice(colon + 1).trim()];
      out.push({ tag: tag[1].toLowerCase() === "confirmed" ? "confirmed" : "plausible", place, quote: "", text: words, evidence: "" });
      into = "words";
      continue;
    }

    const f = out.at(-1);
    if (!f || into === "none" || !t) continue;

    const evidence = /^(?:\*\*)?evidence:(?:\*\*)?\s*(.*)$/i.exec(t);
    if (t.startsWith(">") && (!f.quote || !f.text)) f.quote = plain(`${f.quote} ${t.replace(/^>\s?/, "")}`);
    else if (evidence) { f.evidence = evidence[1].trim(); into = "evidence"; }
    else if (into === "evidence") f.evidence = `${f.evidence} ${t}`.trim();
    else f.text = `${f.text} ${t}`.trim();
  }

  return out;
}

/// The claims a challenger's report says held, under its Checked:
/// `- **Changed 2:** held. Read the backoff.` Their places, as it wrote them.
export function parseHeld(text: string): string[] {
  return sectionOf(text, "Checked").split("\n").flatMap(line => {
    const item = /^\s*(?:\d+[.)]|[-*])\s+(.*)$/.exec(line)?.[1].replace(/\*\*/g, "");
    const colon = item?.indexOf(":") ?? -1;
    if (!item || colon < 0 || !/^held\b/i.test(item.slice(colon + 1).trim())) return [];
    return [item.slice(0, colon).trim()];
  });
}

/// The body of `text`'s `## title` section, or "" when it has none.
const sectionOf = (text: string, title: string) =>
  new RegExp(`^##\\s+${title}\\s*$([\\s\\S]*?)(?=^##\\s|(?![\\s\\S]))`, "im").exec(text)?.[1] ?? "";

/// A finding as a review draft on the report at `entry`, which starts `opening`.
export function findingDraft(finding: Finding, id: number, entry: number, opening: string): ReviewComment {
  return { id, entry, quote: finding.quote, place: finding.place, opening, ask: "finding", text: finding.text, finding: { tag: finding.tag, evidence: finding.evidence } };
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
    return Object.fromEntries(Object.entries(raw).map(([key, list]) => [key, Array.isArray(list) ? list.map(heldKept).filter(isChallenge) : []]));
  } catch {
    return {};
  }
}

/// A challenge kept before it noted the claims that held, with none.
const heldKept = (value: unknown) => typeof value === "object" && value !== null && !("held" in value) ? { ...value, held: [] } : value;

const STATES: readonly ChallengeState[] = ["running", "asking", "done", "failed"];

function isChallenge(value: unknown): value is Challenge {
  if (typeof value !== "object" || value === null) return false;
  if (!("entry" in value && typeof value.entry === "number" && "opening" in value && typeof value.opening === "string" && "findings" in value && typeof value.findings === "number")) return false;
  if (!("state" in value && STATES.some(state => state === value.state))) return false;
  if (!("error" in value && typeof value.error === "string")) return false;
  if (!("held" in value && Array.isArray(value.held) && value.held.every(h => typeof h === "string"))) return false;
  if (!("challenger" in value && typeof value.challenger === "object" && value.challenger !== null)) return false;

  const c = value.challenger;
  return "machine" in c && typeof c.machine === "string" && "agent" in c && typeof c.agent === "string" && "session" in c && typeof c.session === "string";
}
