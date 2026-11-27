import { Icon } from "../../ui/icons.tsx";
import { heldMessage, type OutboxRow, outboxNote } from "../../stores/agents/agentqueue.ts";
import { Spinner } from "../../ui/kit.tsx";
import { promptParts, sessionMessageTitle } from "../../stores/agents/promptparts-state.ts";

/// The steers the node holds for the turn, then the messages kept on this device, a sent one
/// among them until the node lists it; `edit` takes one back into the field, or drops it. A
/// message from another session shows its sender and its words.
export function AgentQueue({ items, edit, outbox, editOutbox }: {
  items: readonly string[];
  edit(index: number, keep: boolean): unknown;
  outbox: readonly OutboxRow[];
  editOutbox(id: number, keep: boolean): unknown;
}) {
  const seen = new Map<string, number>();

  return <>{items.map((text, index) => {
    // Keyed by its words: a row keeps its controls as the ones before it leave.
    const nth = seen.get(text) ?? 0;
    seen.set(text, nth + 1);
    const from = promptParts(text).find(part => part.kind === "session");
    const label = from ? sessionMessageTitle(from.name) || "Other session" : "Steer";
    const { words, names } = heldMessage(text);
    return <div key={`${nth}:${text}`} className={from ? "it steer from" : "it steer"}><span className="lbl" title={label}>{label}</span><span className="t" title={from ? from.text : text}>{from ? from.text : words}</span>
      {names.map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">after the current step</span>
      <button data-agq={index} data-act="edit" onClick={() => edit(index, true)}>Edit</button><button data-agq={index} aria-label="Remove" onClick={() => edit(index, false)}><Icon name="x" /></button>
    </div>;
  })}{outbox.map(row => {
    const busy = row.state === "sending" || row.state === "uploading" || row.state === "resuming" || row.state === "sent";
    return <div key={`out-${row.id}`} className={`it out ${row.state}`} data-outbox={row.id}><span className="lbl">Queued</span><span className="t" title={row.text}>{row.text}</span>
      {row.images.map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">{(row.state === "uploading" || row.state === "resuming") && <Spinner />}{outboxNote(row)}</span>
      <button data-act="edit" disabled={busy} onClick={() => editOutbox(row.id, true)}>Edit</button><button aria-label="Remove" disabled={busy} onClick={() => editOutbox(row.id, false)}><Icon name="x" /></button>
    </div>;
  })}</>;
}
