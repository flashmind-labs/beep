import { useLayoutEffect, useRef } from "react";

export function AgentNewShell({ pane, composer, stash }: { pane: boolean; composer: HTMLElement; stash: string }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const current = host.current;
    if (!current) return;
    current.append(composer);
    return () => { composer.remove(); };
  }, [composer]);
  return <>{!pane && <div className="dhead"><span className="tt"><b>Start a coding agent</b><small>It runs on your machine; follow it and answer it here</small></span><button className="dbtn quiet" id="agList">All sessions</button></div>}
    <div className="agc-wrap" id="agcomposer" ref={host} /><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: stash }} />
  </>;
}
