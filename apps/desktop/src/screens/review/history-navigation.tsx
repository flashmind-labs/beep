import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ListBox, ListBoxItem } from "react-aria-components/ListBox";
import { Button, NO_AUTO, SheetHead } from "../../ui/kit.tsx";
import { Icon } from "../../ui/icons.tsx";
import { reason } from "../../ui/format.ts";
import { openSheet } from "../../ui/parts.tsx";
import type { GitWorktree, HistoryPage } from "../../stores/review/history-state.ts";

export function HistoryNavigation({ query, back, refresh, collapsed, toggleSections, search, checkout }: {
  query: string;
  back: () => void;
  refresh: () => void;
  collapsed: boolean;
  toggleSections: () => void;
  search: (query: string) => void;
  checkout: ReactNode;
}) {
  return <><div className="gh-nav-head">
    <Button look="plain" className="gh-back" onClick={back}>Project</Button>
    <b>Git</b>
    <Button look="plain" className="gh-collapse" onClick={refresh}>Refresh</Button>
    <Button look="plain" className="gh-collapse" onClick={toggleSections}>{collapsed ? "Expand all" : "Collapse all"}</Button>
  </div>
    {checkout}
    <label className="gh-search"><input type="search" maxLength={256} {...NO_AUTO} placeholder="Search commit messages" aria-label="Search commit messages" defaultValue={query} onChange={event => search(event.currentTarget.value)} /></label></>;
}

export function CheckoutBranch({ data, worktree, switchTo }: { data: HistoryPage; worktree: GitWorktree; switchTo(branch: string): Promise<void> }) {
  const branches = data.refs.filter(ref => ref.kind === "local" && ref.name !== worktree.branch).map(ref => ref.name);
  const current = worktree.branch ?? "Detached HEAD";
  const open = () => {
    let busy = false;
    const { close, render } = openSheet("Switch branch", "am-sheet gh-switch-sheet", { canClose: () => !busy });
    render(<BranchPicker branches={branches} location={`${data.machine_name} · ${worktree.path}`} current={current}
      close={close} switchTo={switchTo} pending={value => { busy = value; }} />);
  };

  return <div className="gh-checkout">
    <code title={worktree.path} aria-live="polite">{current}</code>
    <Button look="quiet" small aria-haspopup="dialog" disabled={branches.length === 0} onClick={open}>Switch branch</Button>
  </div>;
}

function BranchPicker({ branches, location, current, close, switchTo, pending }: {
  branches: readonly string[]; location: string; current: string; close(): void;
  switchTo(branch: string): Promise<void>; pending(value: boolean): void;
}) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const saving = useRef(false);
  useLayoutEffect(() => { field.current?.focus(); }, []);

  const needle = query.trim().toLowerCase();
  const shown = branches.filter(branch => branch.toLowerCase().includes(needle));
  const save = async () => {
    if (selected === null || saving.current) return;
    saving.current = true;
    pending(true);
    setBusy(true);
    setError(null);

    try {
      await switchTo(selected);
      close();
    } catch (failure) {
      setError(reason(failure));
      saving.current = false;
      pending(false);
      setBusy(false);
    }
  };

  return <>
    <SheetHead title="Switch branch" />
    <p className="am-sub"><code>{current}</code> · {location}</p>
    <input ref={field} type="search" className="am-code" aria-label="Search local branches" placeholder="Search local branches"
      value={query} disabled={busy} {...NO_AUTO} onChange={event => {
        setQuery(event.currentTarget.value); setSelected(null); setError(null);
      }} />
    <ListBox className="dropdown-menu gh-branch-list" aria-label="Local branches" selectionMode="single" selectionBehavior="replace"
      selectedKeys={selected === null ? [] : [selected]} disabledKeys={busy ? shown : []} onSelectionChange={keys => {
        setSelected(shown.find(branch => keys !== "all" && keys.has(branch)) ?? null); setError(null);
      }} renderEmptyState={() => <p className="gh-empty">No local branches match.</p>}>
      {shown.map(branch => <ListBoxItem className="dropdown-option" key={branch} id={branch} textValue={branch}>
        <span className="dropdown-check"><Icon name="check" /></span><code>{branch}</code>
      </ListBoxItem>)}
    </ListBox>
    {error !== null && <p className="am-msg bad gh-switch-error" role="alert">{error}</p>}
    <div className="gp-foot">
      <Button look="quiet" disabled={busy} onClick={close}>Cancel</Button>
      <Button look="primary" disabled={selected === null || busy} onClick={() => void save()}>{busy ? "Switching…" : "Switch branch"}</Button>
    </div>
  </>;
}
