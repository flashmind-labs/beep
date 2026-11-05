// The Assistant page's chat on this Mac: what a reply streams into a turn, and a saved chat drawn
// back. It draws with the pieces the coding agents' logs and the bar's chat use (`makeTurn`,
// `drawWork`, `reply`, the rows of `turnrows.ts`); `follow` in chat/chat.ts is not used, because
// it is tied to the bar's open session, its queue and its hints.
import { drawWork, makeTurn, reply, type BarEvent } from "../chat/chat.ts";
import { answerHtml, errorCardHtml } from "../agents/turnrows.ts";
import { h } from "../ui/dom.ts";
import { Channel } from "../ui/tauri.ts";
import type { Turn } from "../ui/turn.ts";
import type { StoredTurn } from "./chats-state.ts";

/// Lets the thought end once something else comes.
function endThought(turn: Turn) {
  if (turn.thought && turn.thought.ms == null) turn.thought.ms = Date.now() - turn.thought.start;
}

/// Draws `turn` as finished.
export function endTurn(turn: Turn) {
  turn.done = true;
  turn.ended = Date.now();
  endThought(turn);
  for (const s of turn.steps) if (s.state === "run") s.state = "stop";
  turn.folded = true;
  drawWork(turn);
}

/// A turn for `question` that waits for its reply.
export function startTurn(question: string) {
  const turn = makeTurn(question);
  turn.user = true;
  turn.begun = Date.now();
  turn.done = false;
  drawWork(turn);
  return turn;
}

/// Shows `message` under the turn, as a failed reply does.
export function failTurn(turn: Turn, message: string) {
  turn.failed = true;
  turn.append(h(errorCardHtml(message)));
}

/// A channel that draws one run's events into `turn`. `scroll` runs after each change, so the body
/// can follow the newest text; `finished` once, when the run ends in any way.
export function streamInto(turn: Turn, scroll: () => void, finished: () => void) {
  const channel = new Channel<BarEvent>();
  channel.onmessage = ev => {
    switch (ev.kind) {
      case "text":
        endThought(turn);
        reply(turn).innerHTML = answerHtml(ev.html);
        drawWork(turn);
        break;
      case "reasoning":
        if (!turn.thought || turn.thought.ms != null) turn.thought = { text: turn.thought?.text ?? "", start: Date.now() };
        turn.thought.text += ev.delta;
        drawWork(turn);
        break;
      case "tool_started":
        endThought(turn);
        turn.steps.push({ id: ev.call_id, name: ev.name, label: ev.description, state: "run" });
        drawWork(turn);
        break;
      case "tool_result": {
        const step = turn.steps.find(s => s.id === ev.call_id);
        if (step) step.state = ev.success ? "ok" : "bad";
        drawWork(turn);
        break;
      }
      case "error":
        failTurn(turn, ev.message);
        break;
      case "finished":
        turn.status = ev.status;
        if (ev.status === "cancelled") turn.append(h('<div class="dmuted dstopped">Stopped</div>'));
        endTurn(turn);
        finished();
        break;
      default:
        return;
    }
    scroll();
  };
  return channel;
}

/// A turn read from a saved chat.
export function savedTurn(t: StoredTurn) {
  const turn = makeTurn(t.question);
  turn.steps = t.steps.map(s => ({ id: s.call_id, name: s.name, label: s.label, state: s.ok ? "ok" : "bad" }));
  if (t.reasoning) turn.thought = { text: t.reasoning, start: 0, ms: undefined };
  if (t.answer_html) reply(turn).innerHTML = answerHtml(t.answer_html);
  endTurn(turn);
  return turn;
}
