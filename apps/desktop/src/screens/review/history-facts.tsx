interface HistoryFactsProps {
  id: string;
  author: string | null;
  parents: readonly string[];
  onParent: (id: string) => void;
}

export function HistoryFacts({ id, author, parents, onParent }: HistoryFactsProps) {
  return <div className="gh-facts">
    <span>{author === null ? "Worktree" : "Commit"}</span><code>{id}</code>
    {author !== null && <>
      <span>Author</span><span>{author}</span><span>Parents</span>
      <span>{parents.length ? parents.map(parent => <button key={parent} type="button" className="gh-parent" onClick={() => onParent(parent)}>{parent.slice(0, 7)}</button>) : "Initial commit"}</span>
    </>}
  </div>;
}
