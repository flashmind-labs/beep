import type { ReactNode } from "react";

export function ResearchShell({ head, chat, tabs, pane }: { head: ReactNode; chat: ReactNode; tabs: ReactNode; pane: ReactNode }) {
  return <div className="rs"><div className="dhead">{head}</div><div className="agshell"><div className="agconv"><div className="dbody rs-log"><div className="dagmain"><div className="dcoderlog">{chat}</div></div></div><div className="dinput small"><textarea rows={1} autoComplete="off" spellCheck={false} aria-label="Message the lead" /></div></div><div className="agdiv" role="separator" aria-orientation="vertical" tabIndex={0} title="Drag to resize, double-click to reset" /><aside className="agpane rs-pane"><div className="agpane-content"><div className="agphead"><div className="agtabs" role="tablist">{tabs}</div></div><div className="agpviews">{pane}</div></div></aside></div></div>;
}
