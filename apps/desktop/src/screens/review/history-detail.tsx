import { useLayoutEffect, useRef, type ReactNode } from "react";

/// The diff is a view kept across detail updates, with its own root element; React keeps its host stable.
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
