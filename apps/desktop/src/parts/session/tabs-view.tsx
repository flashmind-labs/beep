import type { MouseEvent, PointerEvent, Ref } from "react";
import { Icon, type IconName } from "../../ui/icons.tsx";

export interface PaneTabItem { id: string; name: string; icon: IconName; count: number | null; closeIcon: IconName | null }

export function PhonePaneTabs({ tabs, selected, select }: { tabs: readonly { id: string; name: string }[]; selected: string; select(id: string): void }) {
  return <>{tabs.map(tab => <button key={tab.id} type="button" data-mnav={tab.id} aria-selected={tab.id === selected} onClick={() => select(tab.id)}>{tab.name}</button>)}</>;
}

/// What a dock's tab strip does with its tabs.
export interface PaneTabActions {
  pick(id: string): void;
  close(id: string): void;
  menu(event: MouseEvent, id: string): void;
  down(event: PointerEvent<HTMLButtonElement>, id: string): void;
}

export function PaneTabs({ tabs, selected, dragging, actions }: { tabs: readonly PaneTabItem[]; selected: string; dragging: string | null; actions: PaneTabActions }) {
  return <>{tabs.map(tab => <PaneTab key={tab.id} tab={tab} className={`agtab${tab.id === selected ? " on" : ""}${tab.id === dragging ? " dragging" : ""}`} selected={tab.id === selected} actions={actions} />)}</>;
}

/// One tab of a strip, or with no actions the ghost of one being dragged.
export function PaneTab({ tab, className, selected, actions, ref }: { tab: PaneTabItem; className: string; selected: boolean; actions?: PaneTabActions; ref?: Ref<HTMLButtonElement> }) {
  return <button ref={ref} type="button" className={className} role="tab" aria-selected={selected} data-mtab={tab.id} title={tab.name}
    onClick={actions && (() => actions.pick(tab.id))} onContextMenu={actions && (event => actions.menu(event, tab.id))} onPointerDown={actions && (event => actions.down(event, tab.id))}
    onMouseDown={event => { if (event.button === 1) event.preventDefault(); }} onAuxClick={actions && tab.closeIcon !== null ? (event => { if (event.button === 1) actions.close(tab.id); }) : undefined}>
    <i style={{ display: "contents" }}><Icon name={tab.icon} /></i><span>{tab.name}</span>{tab.count !== null && <small>{tab.count}</small>}{tab.closeIcon !== null && <em data-mclose={tab.id} aria-label={`Close ${tab.name}`} title="Close (⌘W)"
      onClick={actions && (event => { event.stopPropagation(); actions.close(tab.id); })} onPointerDown={event => event.stopPropagation()}><Icon name={tab.closeIcon} /></em>}
  </button>;
}
