import { Spinner } from "../../ui/kit.tsx";
import { grouped, type DiffView } from "../../stores/review/diff-state.ts";

export function DiffMessage({ loading, view }: { loading: boolean; view: DiffView | null }) {
  return <>{!view && loading && <div className="dv-empty"><Spinner />Reading the changes…</div>}{view && !view.files.length && <div className="dv-empty"><b>Nothing changed</b><span>{view.working_tree ? `The folder matches ${view.base}, with nothing uncommitted.` : `The branch has no commits past ${view.base}.`}</span></div>}</>;
}

export function DiffMore({ count }: { count: number }) { return count ? <div className="dv-more">And {grouped(count)} more files not shown.</div> : null; }
export function DiffKeys() { return <><span><kbd>j</kbd><kbd>k</kbd>next, previous file</span><span><kbd>x</kbd>fold the file</span><span><kbd>/</kbd>filter</span><span><kbd>n</kbd><kbd>N</kbd>next, previous match</span></>; }
