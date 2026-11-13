import { useRef } from "react";
import { openMenu } from "../ui/menu.ts";
import { CHEV, openEditor, type EditorRow } from "./editors.ts";
import { Button } from "../ui/kit.tsx";

export function ProjectEditors({ editors, folder }: { editors: EditorRow[]; folder: string }) {
  const main = useRef<HTMLButtonElement>(null);
  const [first, ...rest] = editors;
  if (!first) return null;
  const open = (editor: EditorRow) => { if (main.current) void openEditor(editor, folder, main.current); };
  return <><Button look="quiet" small title={folder} ref={main} onClick={() => open(first)}>Open in {first.name}</Button>{!!rest.length && <Button look="quiet" small title="Other editors" aria-label="Other editors" onClick={event => openMenu(event.nativeEvent, "Open in", rest.map(editor => ({ label: editor.name, on: () => open(editor) })))} dangerouslySetInnerHTML={{ __html: CHEV }} />}</>;
}
