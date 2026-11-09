import { useLayoutEffect, useRef } from "react";

export interface ComposerChoicesState {
  machine: string;
  adding: boolean;
  machineDisabled: boolean;
  folder: string;
  folderSub: string;
  folderDisabled: boolean;
  branch: string;
  showBranch: boolean;
  showClear: boolean;
}
interface Actions {
  machine: (event: MouseEvent) => void;
  folder: (event: MouseEvent) => void;
  branch: (value: string) => void;
  clear: () => void;
}

export function ComposerChoices({ value, actions }: { value: ComposerChoicesState; actions: Actions }) {
  const branch = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (branch.current && document.activeElement !== branch.current && branch.current.value !== value.branch) branch.current.value = value.branch;
  });
  return <>
    <button type="button" className="pill" data-composer="machine" title="The machine it runs on (Shift+Tab in the task field)" disabled={value.machineDisabled} onClick={event => actions.machine(event.nativeEvent)}>{value.machine}{value.adding && <> <small>adding</small></>}</button>
    <button type="button" className="pill" data-composer="folder" title="The checkout, a new worktree, or an existing worktree" disabled={value.folderDisabled} onClick={event => actions.folder(event.nativeEvent)}>{value.folder} <small>{value.folderSub}</small></button>
    <span className="choices" data-composer="extra"/>
    <input ref={branch} className="branch" type="text" spellCheck={false} autoComplete="off" aria-label="Branch name" title="The new branch, in a new worktree" hidden={!value.showBranch} defaultValue="" onChange={event => actions.branch(event.currentTarget.value)}/>
    <button type="button" className="dbtn quiet small" title="Back to a plain start" hidden={!value.showClear} onClick={actions.clear}>Clear</button>
  </>;
}
