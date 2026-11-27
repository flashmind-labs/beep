import { StashBar } from "./stash-hints.tsx";
import { useLayoutEffect, useRef } from "react";
import { Button } from "../../ui/kit.tsx";

export function AgentNewShell({ pane, composer, project, menuOpen, openList }: { pane: boolean; composer: HTMLElement; project: string | null; menuOpen: boolean; openList(): void }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const current = host.current;
    if (!current) return;
    current.append(composer);
    return () => { composer.remove(); };
  }, [composer]);
  return <>{!pane && <div className="dhead"><span className="tt"><b>Start a coding agent</b><small>It runs on your machine; follow it and answer it here</small></span><Button look="quiet" id="agList" onClick={openList}>All sessions</Button></div>}
    <div className="agc-wrap" id="agcomposer" ref={host} /><StashBar project={project} away={menuOpen} />
  </>;
}
