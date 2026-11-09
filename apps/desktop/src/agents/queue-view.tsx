import type { Waiting } from "./agentqueue.ts";

const ids = new WeakMap<Waiting, number>();
let nextId = 0;
function key(item: Waiting) {
  let id = ids.get(item);
  if (id === undefined) { id = ++nextId; ids.set(item, id); }
  return id;
}

export function AgentQueue({ items }: { items: readonly Waiting[] }) {
  return <>{items.map((item, index) => {
    const text = item.typed || (item.images.length || item.paths.length ? "" : item.text);
    return <div key={key(item)} className="it steer"><span className="lbl">Steer</span><span className="t" title={item.text}>{text}</span>
      {[...item.images.map(image => image.name), ...item.paths].map((name, i) => <span className="qatt" title={name} key={i}>{name}</span>)}
      <span className="w">{item.state === "sending" ? "sending" : "after the current step"}</span>
      <button data-agq={index} data-act="edit">Edit</button><button data-agq={index} aria-label="Remove"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M6 6l12 12M18 6L6 18"/></svg></button>
    </div>;
  })}</>;
}
