// Approval cards in a chat's turns, and answering them.
import { turnState } from "./turn-state.ts";
import { discardChatApproval, renderChatApproval, renderChatNotice } from "./work-view.tsx";
import { entry, live, S, stopping } from "../bar/core.ts";
import { AnsweredIds } from "./approvals.ts";
import type { LiveEntry, PendingApproval } from "../bar/core.ts";
import { refreshHints } from "../bar/hints.ts";
import { invoke } from "../ui/tauri.ts";
import type { Turn } from "../ui/turn.ts";
import { owningChatBody } from "./turns.ts";
import { follow, runTurn } from "./runs.ts";

/// One line of an approval card.
export interface ApprovalField {
  label: string;
  value: string;
}

/// An action waiting for the user's OK (`Asking`).
export interface Asking {
  query_id: string;
  app: string;
  action: string;
  fields: ApprovalField[];
}

/// The user's answer to an approval card.
export type Decision = "allow" | "session" | "always" | "deny";

/// A routine asking for a grant (`request_grant`). A grant always ends, so
/// its card has no Always, and the Always key allows it.
export const forRoutine = (action: string | null | undefined) => /for a routine$/.test(action ?? "");

/// Turn approvals answered in this window.
export const answeredTurns = new AnsweredIds();

export function approvalCard(id: string | null, turn: Turn, { query_id, app, action, fields }: Asking) {
  if (answeredTurns.has(query_id)) return;
  const grant = forRoutine(action);
  const card = document.createElement("div");
  card.className = "dcard";
  const owner = owningChatBody(turn);
  if (!owner) return;
  renderChatApproval(owner, turn, card, { app, action, fields, grant, answer: decision => void answer(decision) });
  turn.append(card);
  const e = entry(id);
  e.approval = { queryId: query_id, turn, card, grant };
  e.body ??= owningChatBody(turn);
  if (id === S.session) refreshHints();
}

/// Takes chat `id`'s approval card away, saying how it was answered, and
/// readies its turn for the run the answer resumes.
/// Replace a settled approval with a notice owned by its retained chat body.
export function approvalNotice(turn: Turn, card: HTMLElement, message: string) {
  discardChatApproval(turn);
  const owner = owningChatBody(turn);
  if (!owner) { card.remove(); return; }
  const box = document.createElement("div");
  box.className = "dmuted";
  renderChatNotice(owner, turn, box, { kind: "label", message });
  card.replaceWith(box);
}

export function settle(id: string | null, e: LiveEntry, pending: PendingApproval, decision: Decision) {
  e.approval = null;
  answeredTurns.mark(pending.queryId, id);
  const said = pending.grant
    ? { allow: "Allowed for this routine", deny: "Denied" }[decision as "allow" | "deny"]
    : { allow: "Allowed once", session: "Allowed for the rest of this chat", always: "Allowed, and always allowed from now on", deny: "Denied" }[decision];
  const turn = pending.turn;
  approvalNotice(turn, pending.card, String(said));
  turnState(turn).done = false;
  turnState(turn).ended = null;
  e.run = { id: null, turn };
  if (id === S.session) refreshHints();
  return turn;
}

export async function answer(decision: Decision) {
  const id = S.session, e = live.get(id), pending = e?.approval;
  if (!e || !pending) return;
  if (pending.grant && (decision === "always" || decision === "session")) decision = "allow";
  const turn = settle(id, e, pending, decision);
  await stopping.get(id);
  const answered = invoke("answer_approval", { session: id, queryId: pending.queryId, decision, onEvent: follow(id, turn) })
    // Not answered after all: the card may come back with the chat.
    .catch(err => { answeredTurns.forget(pending.queryId); throw err; });
  await runTurn(id, turn, answered);
}
