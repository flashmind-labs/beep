import { Fragment } from "react";
import { agentOf, count, type Entry, type Note, type Open, type Said } from "./research-state.ts";
import { ResearchSources } from "./sources-view.tsx";
import { Avatar } from "./avatar-view.tsx";

export function ResearchLivePane({ research, tab }: { research: Open; tab: "notes" | "sources" | "room" }) {
  if (tab === "sources") return <ResearchSources research={research.research} entries={research.entries}/>;
  if (tab === "notes") return <Notes research={research}/>;
  return <Room research={research}/>;
}

function Notes({ research }: { research: Open }) {
  const notes = research.entries.filter((entry): entry is Entry & Note => entry.kind === "note");
  return <div className="rpane">{notes.length ? <div className="rbox">{notes.map(note => <div key={note.seq} className="rmsg">
    <Avatar id={note.agent}/><b>{agentOf(research, note.agent).name}</b><p>{note.content}{note.sources.length > 0 && <> <Citations research={research} numbers={note.sources}/></>}</p>
  </div>)}</div> : <div className="rbox rempty">No notes yet.</div>}</div>;
}

function Citations({ research, numbers }: { research: Open; numbers: number[] }) {
  return <sup>{numbers.map((number, index) => {
    const source = research.research?.sources.find(item => item.number === number);
    return <Fragment key={`${number}:${index}`}>{index > 0 && ", "}{source ? <a className="rcite" href={source.url} title={source.title}>{number}</a> : number}</Fragment>;
  })}</sup>;
}

function Room({ research }: { research: Open }) {
  const messages = research.entries.filter((entry): entry is Entry & Said => entry.kind === "said");
  return <><div className="rhd"><b>Room</b><small>{count(messages.length, "message")}</small></div>
    <div className="rpane thread">{messages.length ? messages.map(message => <div className="rmsg" key={message.seq}>
      <Avatar id={message.agent}/><b>{agentOf(research, message.agent).name}{message.to && <> <span className="to">to {agentOf(research, message.to).name}</span></>}</b><p>{message.content}</p>
    </div>) : <div className="dmuted">Nobody has said anything yet.</div>}</div></>;
}
