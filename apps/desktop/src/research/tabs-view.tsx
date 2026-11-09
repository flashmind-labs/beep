import { agentOf, shownTab, type Open } from "./research-state.ts";

export function ResearchTabs({ research }: { research: Open }) {
  const selected = shownTab(research);
  const notes = research.entries.filter(entry => entry.kind === "note").length;
  const tabs = [
    { id: "draft", label: "Draft", count: 0, close: false },
    { id: "notes", label: "Notes", count: notes, close: false },
    { id: "sources", label: "Sources", count: research.research?.sources.length ?? 0, close: false },
    { id: "agents", label: "Agents", count: 0, close: false },
    ...research.tabs.map(id => ({ id, label: id === "room" ? "Room" : agentOf(research, id).name, count: 0, close: true })),
  ];
  return <>{tabs.map(tab => <button key={tab.id} className={`agtab${selected === tab.id ? " on" : ""}`} role="tab" aria-selected={selected === tab.id} data-act={`tab:${tab.id}`} title={tab.label}>
    <span>{tab.label}</span>{tab.count > 0 && <small>{tab.count}</small>}
    {tab.close && <em role="button" data-act={`close:${tab.id}`} aria-label={`Close ${tab.label}`} title="Close"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M6 6l12 12M18 6L6 18"/></svg></em>}
  </button>)}</>;
}
