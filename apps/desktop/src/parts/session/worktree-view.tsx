import { Checkbox, NO_AUTO } from "../../ui/kit.tsx";
interface Item { path: string; title: string; name: string; sub: string; visible: boolean }
export interface WorktreeMenuModel {
  label: string; items: Item[]; shown: string; follow: boolean; open: boolean; search: string;
  /// Every worktree of the repository is listed, not only the session's.
  all: boolean;
  pick(path: string | null): void; searchChanged(value: string): void; showAll(all: boolean): void; toggle(): void;
}

export function WorktreeMenu(p: WorktreeMenuModel) {
  return <>
    <button className="agpbtn agwt" aria-haspopup="menu" aria-expanded={p.open} title="The worktree Files and Changes show" onClick={p.toggle}><code>{p.label}</code><span aria-hidden="true">▾</span></button>
    {p.open && <div className="agwtm" role="menu">
      <button role="menuitemcheckbox" aria-checked={p.follow} data-wt="follow" onClick={() => p.pick(null)}><b>Follow the agent</b></button>
      <hr />
      <div className="agwt-find">
        <input type="search" className="agwt-search" autoFocus {...NO_AUTO} aria-label="Search worktrees" placeholder="Search worktrees" value={p.search} onInput={e => p.searchChanged(e.currentTarget.value)} onChange={() => {}} />
        <label className="agwt-all"><Checkbox checked={p.all} change={p.showAll} />Show all</label>
      </div>
      {p.items.map(item => <button key={item.path} hidden={!item.visible} role="menuitemradio" aria-checked={item.path === p.shown} data-wt={item.path} title={item.title} onClick={() => p.pick(item.path)}><code>{item.name}</code><small>{item.sub}</small></button>)}
      <small className="agwt-empty" hidden={p.items.some(item => item.visible)}>No matching worktrees</small>
    </div>}
  </>;
}
