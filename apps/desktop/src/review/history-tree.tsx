import type { ReactNode } from "react";

interface TreeNodeProps {
  nodeKey: string;
  label: ReactNode;
  open: boolean;
  commit?: boolean;
  title?: string;
  current?: boolean;
  onToggle: (open: boolean) => void;
  onSelect?: () => void;
  children?: ReactNode;
}

export function TreeNode({ nodeKey, label, open, commit = false, title, current, onToggle, onSelect, children }: TreeNodeProps) {
  return <details className={`gh-node${commit ? " gh-commit" : ""}`} data-key={nodeKey} open={open}
    onToggle={event => { if (event.currentTarget.open !== open) onToggle(event.currentTarget.open); }}>
    <summary title={title} aria-current={current} onClick={onSelect}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m6 4 4 4-4 4" /></svg>{label}
    </summary>
    <div className="gh-children">{open ? children : null}</div>
  </details>;
}
