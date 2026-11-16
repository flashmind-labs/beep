import { useLayoutEffect, useRef } from "react";
import { Button } from "../ui/kit.tsx";

export interface ComposerChoicesState {
  machine: string;
  /// The machine is picked and runs agents.
  on: boolean;
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

export function ComposerMachine({ value, open }: { value: ComposerChoicesState; open: Actions["machine"] }) {
  return <button type="button" className="pill" data-composer="machine" title="The machine it runs on (Shift+Tab in the task field)" disabled={value.machineDisabled} onClick={event => open(event.nativeEvent)}>{value.on && <span className="ddot on" />}{value.machine}{value.adding && <> <small>adding</small></>}</button>;
}

export function ComposerChoices({ value, actions }: { value: ComposerChoicesState; actions: Actions }) {
  const branch = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (branch.current && document.activeElement !== branch.current && branch.current.value !== value.branch) branch.current.value = value.branch;
  });
  return <>
    <button type="button" className="pill" data-composer="folder" title="The checkout, a new worktree, or an existing worktree" disabled={value.folderDisabled} onClick={event => actions.folder(event.nativeEvent)}>{value.folder} <small>{value.folderSub}</small></button>
    <span className="choices" data-composer="extra"/>
    <input ref={branch} className="branch" type="text" spellCheck={false} autoComplete="off" aria-label="Branch name" title="The new branch, in a new worktree" hidden={!value.showBranch} defaultValue="" onChange={event => actions.branch(event.currentTarget.value)}/>
    <Button look="quiet" small title="Back to a plain start" hidden={!value.showClear} onClick={actions.clear}>Clear</Button>
  </>;
}
