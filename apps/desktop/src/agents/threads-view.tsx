// A chat's threads, listed above its messages: each by name, how it stands
// and what it asks or last said. A row opens the thread.
import type { ReactNode } from "react";
import { Button, LineRow, SectionHead, Spinner } from "../ui/kit.tsx";
import type { AgentSession } from "./agents.ts";
import { sessionKey } from "./conversation-state.ts";
import type { Thread, ThreadDetail } from "./threads-state.ts";

export function ThreadList({ threads, onOpen, onNew }: { threads: readonly Thread[]; onOpen(row: AgentSession): void; onNew(): void }): ReactNode {
  if (!threads.length) return null;

  return <div className="ag-threads">
    <SectionHead title="Threads" count={threads.length}><Button look="quiet" small onClick={onNew}>New thread</Button></SectionHead>
    {threads.map(thread => <LineRow key={sessionKey(thread.row)} dataKey={sessionKey(thread.row)} lead={null} title={thread.name}
      sub={<Detail detail={thread.detail} />} right={<Standing thread={thread} />} onOpen={() => onOpen(thread.row)} />)}
  </div>;
}

function Detail({ detail }: { detail: ThreadDetail }) {
  if (!detail) return null;
  if (detail.kind === "question") return detail.text;
  return <span className="nt-md" dangerouslySetInnerHTML={{ __html: detail.html }} />;
}

function Standing({ thread }: { thread: Thread }) {
  switch (thread.state) {
    case "working": return <Spinner dot title={thread.row.doing ?? undefined} label="Working" />;
    case "asking": return <span className="dstate ask">Needs you</span>;
    case "done": return <span className="dstate">Done</span>;
    case "ended": return <span className="dstate">Ended</span>;
  }
}
