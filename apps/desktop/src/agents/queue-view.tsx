import { Icon } from "../ui/icons.tsx";
import { identity } from "../ui/parts.tsx";
import { type OutboxRow, outboxNote, type Waiting } from "./agentqueue.ts";
import { Spinner } from "../ui/kit.tsx";
import { promptParts, sessionMessageTitle } from "./promptparts-state.ts";


/// What a waiting row says after its text.
function queueNote(state: Waiting["state"]) {
  switch (state) {
    case "waiting": return "waiting";
    case "sending": return "sending";
    default: return "after the current step";
  }
}

/// The steers waiting for the turn, then the messages waiting on this device
/// for the machine; `edit` takes one back into the field, or drops it. A
/// message from another session shows its sender and its words.
export function AgentQueue({ items, edit, outbox, editOutbox }: {
  items: readonly Waiting[];
  edit(index: number, keep: boolean): unknown;
  outbox: readonly OutboxRow[];
  editOutbox(id: number, keep: boolean): unknown;
}) {
  return <>{items.map((item, index) => {
    const from = promptParts(item.text).find(part => part.kind === "session");
    const label = from ? sessionMessageTitle(from.name) || "Other session" : "Steer";
    const text = from ? from.text : item.typed || (item.images.length || item.paths.length ? "" : item.text);
    return <div key={identity(item)} className={from ? "it steer from" : "it steer"}><span className="lbl" title={label}>{label}</span><span className="t" title={from ? from.text : item.text}>{text}</span>
      {[...item.images.map(image => image.name), ...item.paths].map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">{queueNote(item.state)}</span>
      <button data-agq={index} data-act="edit" onClick={() => edit(index, true)}>Edit</button><button data-agq={index} aria-label="Remove" onClick={() => edit(index, false)}><Icon name="x" /></button>
    </div>;
  })}{outbox.map(row => {
    const busy = row.state === "sending" || row.state === "uploading";
    return <div key={`out-${row.id}`} className={`it out ${row.state}`} data-outbox={row.id}><span className="lbl">Queued</span><span className="t" title={row.text}>{row.text}</span>
      {row.images.map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">{row.state === "uploading" && <Spinner />}{outboxNote(row)}</span>
      <button data-act="edit" disabled={busy} onClick={() => editOutbox(row.id, true)}>Edit</button><button aria-label="Remove" disabled={busy} onClick={() => editOutbox(row.id, false)}><Icon name="x" /></button>
    </div>;
  })}</>;
}
