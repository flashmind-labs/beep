import { discardChatWork } from "./work-view.tsx";
import type { ChatBody, Turn } from "../ui/turn.ts";

// The order belongs to the conversation, even while its body is detached.
// Turn contents have their own owners and must be released before removal.
export class Conversation {
  private items: Turn[];
  private version = 0;
  private readonly body: ChatBody;

  constructor(body: ChatBody) {
    this.body = body;
    this.items = [...body.children].filter((element): element is Turn => element.tagName === "DIV" && element.classList.contains("turn"));
  }

  get turns(): readonly Turn[] { return this.items; }
  get revision() { return this.version; }
  invalidate() { this.version++; }

  append(turn: Turn) { this.replace([...this.items, turn]); }
  prepend(turns: readonly Turn[]) { this.replace([...turns, ...this.items]); }
  dropFrom(turn: Turn) {
    const index = this.items.indexOf(turn);
    if (index >= 0) this.replace(this.items.slice(0, index));
  }

  replace(turns: readonly Turn[]) {
    if (turns.length === this.items.length && turns.every((turn, index) => turn === this.items[index])) return;
    this.version++;
    const keep = new Set(turns);
    for (const turn of this.items) if (!keep.has(turn)) { discardChatWork(turn); turn.remove(); }
    let previous: Turn | null = null;
    for (const turn of turns) {
      if (previous) {
        if (previous.nextSibling !== turn) previous.after(turn);
      } else {
        const first = [...this.body.children].find(element => element.classList.contains("turn"));
        if (first !== turn) first ? first.before(turn) : this.body.append(turn);
      }
      previous = turn;
    }
    this.items = [...turns];
  }
}

const conversations = new WeakMap<ChatBody, Conversation>();
export function conversation(body: ChatBody) {
  let current = conversations.get(body);
  if (!current) { current = new Conversation(body); conversations.set(body, current); }
  return current;
}
