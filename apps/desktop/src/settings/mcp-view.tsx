import type { McpServer, McpTools } from "./apps.ts";

interface Props {
  server: McpServer; tools: McpTools | null; error: string; asking: string | null; confirming: boolean; busy: boolean;
  toggle(name: string): void; confirmTool(): void; cancelTool(): void; confirmRemove(): void; cancelRemove(): void; remove(): void;
}
const label = (name: string) => name.replace(/[_-]/g, " ").trim().replace(/^./, c => c.toUpperCase());
export function McpView(p: Props) {
  const tools = p.tools, count = tools?.tools.filter(tool => tool.enabled).length ?? 0;
  return <><div className="dhead"><span className="dalogo">{p.server.name.slice(0, 1).toUpperCase()}</span><span className="tt"><b>{p.server.name}</b><small className="dmono">{p.server.url}</small></span></div><div className="dlist dapps" id="tlist">
    {p.error ? <div className="derr dpad">{p.error}</div> : !tools && <div className="dmuted dpad"><span className="dspin" /></div>}
    {tools && <><div className="dsec dsplit"><span>Tools</span><span className={count > tools.limit ? "over" : undefined}>{count} of {tools.limit} on</span></div>{tools.tools.map(tool => {
      const name = label(tool.name);
      if (p.asking === tool.name) return <div className="daction" key={tool.name}><span>{name}<small>More than {tools.limit} tools makes replies slower and tool picks worse</small></span><button className="dbtn quiet" onClick={p.cancelTool}>Cancel</button><button className="dbtn pri" disabled={p.busy} onClick={p.confirmTool}>Turn on</button></div>;
      return <div className={`daction${tool.usable ? "" : " off"}`} title={tool.description || undefined} key={tool.name}><span>{name}{!tool.usable && <small>Too large for Beep to use</small>}</span><button className="dswitch" type="button" role="switch" aria-label={`Use ${name}`} aria-checked={tool.usable && tool.enabled} disabled={!tool.usable || p.busy} onClick={() => p.toggle(tool.name)} /></div>;
    })}<div className="dmuted dpad dblock">Beep uses up to {tools.limit} tools from a server, so turn on the ones you need. It asks before every tool on a server you add.</div></>}
    <div className="dpad dline">{p.confirming ? <><span className="dmuted">Beep stops using this server</span><button className="dbtn quiet" disabled={p.busy} onClick={p.cancelRemove}>Cancel</button><button className="dbtn warn" disabled={p.busy} onClick={p.remove}>Remove</button></> : <button className="dbtn warn" disabled={p.busy} onClick={p.confirmRemove}>Remove server</button>}</div>
  </div></>;
}
