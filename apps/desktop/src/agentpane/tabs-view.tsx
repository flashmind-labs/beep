export interface PaneTabItem { id: string; name: string; icon: string; count: number | null; closeIcon: string | null }

export function PhonePaneTabs({ tabs, selected, select }: { tabs: readonly { id: string; name: string }[]; selected: string; select(id: string): void }) {
  return <>{tabs.map(tab => <button key={tab.id} type="button" data-mnav={tab.id} aria-selected={tab.id === selected} onClick={() => select(tab.id)}>{tab.name}</button>)}</>;
}

export function PaneTabs({ tabs, selected }: { tabs: readonly PaneTabItem[]; selected: string }) {
  return <>{tabs.map(tab => <button key={tab.id} type="button" className={`agtab${tab.id === selected ? " on" : ""}`} role="tab" aria-selected={tab.id === selected} data-mtab={tab.id} title={tab.name}>
    <i style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: tab.icon }} /><span>{tab.name}</span>{tab.count !== null && <small>{tab.count}</small>}{tab.closeIcon !== null && <em data-mclose={tab.id} aria-label={`Close ${tab.name}`} title="Close (⌘W)" dangerouslySetInnerHTML={{ __html: tab.closeIcon }} />}
  </button>)}</>;
}
