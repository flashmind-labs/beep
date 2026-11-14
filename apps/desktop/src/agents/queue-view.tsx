import { Icon } from "../ui/icons.tsx";
import { identity } from "../ui/parts.ts";
import { type OutboxRow, outboxNote, type Waiting } from "./agentqueue.ts";


/// The steers waiting for the turn, then the messages waiting on this device
/// for the machine; `edit` takes one back into the field, or drops it.
export function AgentQueue({ items, edit, outbox, editOutbox }: {
  items: readonly Waiting[];
  edit(index: number, keep: boolean): unknown;
  outbox: readonly OutboxRow[];
  editOutbox(id: number, keep: boolean): unknown;
}) {
  return <>{items.map((item, index) => {
    const text = item.typed || (item.images.length || item.paths.length ? "" : item.text);
    return <div key={identity(item)} className="it steer"><span className="lbl">Steer</span><span className="t" title={item.text}>{text}</span>
      {[...item.images.map(image => image.name), ...item.paths].map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">{item.state === "sending" ? "sending" : "after the current step"}</span>
      <button data-agq={index} data-act="edit" onClick={() => edit(index, true)}>Edit</button><button data-agq={index} aria-label="Remove" onClick={() => edit(index, false)}><Icon name="x" /></button>
    </div>;
  })}{outbox.map(row => {
    const busy = row.state === "sending";
    return <div key={`out-${row.id}`} className={`it out ${row.state}`} data-outbox={row.id}><span className="lbl">Queued</span><span className="t" title={row.text}>{row.text}</span>
      {row.images.map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">{outboxNote(row)}</span>
      <button data-act="edit" disabled={busy} onClick={() => editOutbox(row.id, true)}>Edit</button><button aria-label="Remove" disabled={busy} onClick={() => editOutbox(row.id, false)}><Icon name="x" /></button>
    </div>;
  })}</>;
}
