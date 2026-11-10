import type { Waiting } from "./agentqueue.ts";

const ids = new WeakMap<Waiting, number>();
let nextId = 0;
function key(item: Waiting) {
  let id = ids.get(item);
  if (id === undefined) { id = ++nextId; ids.set(item, id); }
  return id;
}

/// The steers waiting for the turn; `edit` takes one back into the field, or drops it.
export function AgentQueue({ items, edit }: { items: readonly Waiting[]; edit(index: number, keep: boolean): unknown }) {
  return <>{items.map((item, index) => {
    const text = item.typed || (item.images.length || item.paths.length ? "" : item.text);
    return <div key={key(item)} className="it steer"><span className="lbl">Steer</span><span className="t" title={item.text}>{text}</span>
      {[...item.images.map(image => image.name), ...item.paths].map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">{item.state === "sending" ? "sending" : "after the current step"}</span>
      <button data-agq={index} data-act="edit" onClick={() => edit(index, true)}>Edit</button><button data-agq={index} aria-label="Remove" onClick={() => edit(index, false)}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M6 6l12 12M18 6L6 18"/></svg></button>
    </div>;
  })}</>;
}
