import { openMenu } from "../../ui/menu.tsx";
import { CHEV, openEditor, type EditorRow } from "./editors.ts";
import { Button } from "../../ui/kit.tsx";

export function ProjectEditors({ editors, folder }: { editors: EditorRow[]; folder: string }) {
  const [first, ...rest] = editors;
  if (!first) return null;
  const open = (editor: EditorRow) => void openEditor(editor, folder);
  return <><Button look="quiet" small title={folder} onClick={() => open(first)}>Open in {first.name}</Button>{!!rest.length && <Button look="quiet" small title="Other editors" aria-label="Other editors" onClick={event => openMenu(event.nativeEvent, "Open in", rest.map(editor => ({ label: editor.name, on: () => open(editor) })))} dangerouslySetInnerHTML={{ __html: CHEV }} />}</>;
}
