import { useLayoutEffect, useRef, type ReactNode } from "react";

/// The diff owns virtualized native sections; React keeps its host stable across detail updates.
export function HistoryDetail({ header, body, diff }: { header: ReactNode; body: ReactNode; diff: HTMLElement | null }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const node = host.current;
    if (!node || !diff) return;
    node.append(diff);
    return () => { diff.remove(); };
  }, [diff]);
  return <>{header}<div className="gh-detail-body" ref={host}>{body}</div></>;
}
