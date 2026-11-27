import { useRef, useState } from "react";
import { flushSync } from "react-dom";
import { invoke } from "../../ui/tauri.ts";
import { openSheet } from "../../ui/parts.tsx";
import { reason } from "../../ui/format.ts";
import type { Project } from "../../stores/project/projects-state.ts";
import { Button, SheetHead } from "../../ui/kit.tsx";

export function openHandoff(project: Project, to: string, changed: () => void) {
  let closed = false;
  const { render } = openSheet("Bring uncommitted work", "am-sheet ip-sheet", { onClose: () => { closed = true; } });
  flushSync(() => render(<Handoff project={project} to={to} closed={() => closed} changed={changed} />));
}

function Handoff({ project, to, closed, changed }: { project: Project; to: string; closed(): boolean; changed(): void }) {
  const [status, setStatus] = useState("Choose the machine holding your edits."), [busy, setBusy] = useState(false);
  const pending = useRef(false), destination = project.machines.find(place => place.id === to);
  const bring = async (from: string) => {
    if (pending.current) return;
    pending.current = true; setBusy(true); setStatus("Bringing uncommitted work...");
    try {
      await invoke("project_handoff_work", { project: project.name, from, to });
      if (!closed()) setStatus("Work brought here. The source copy is still intact.");
      changed();
    } catch (error) {
      if (!closed()) { pending.current = false; setBusy(false); setStatus(reason(error)); }
    }
  };
  return <><SheetHead title="Bring uncommitted work" />
    <div className="am-sub">Destination: {destination?.name ?? "this machine"}. Both checkouts must be at the same commit and the destination must be clean. Edits arrive unstaged; the source copy stays intact. Ignored files stay on their machine.</div>
    <div className="am-sub">{status}</div>{project.machines.filter(place => place.id !== to).map(source => <Button key={source.id} disabled={busy} onClick={() => void bring(source.id)}>Bring work from {source.name}</Button>)}</>;
}
