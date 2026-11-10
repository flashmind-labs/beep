import { useRef } from "react";
import { IC } from "../ui/icons.ts";

export function FilePicker({ attach, disabled = false }: { attach(files: File[]): unknown; disabled?: boolean }) {
  const input = useRef<HTMLInputElement>(null);
  return <><button type="button" className="dbtn quiet" title="Attach files" aria-label="Attach files" disabled={disabled} onPointerDown={event => event.preventDefault()} onClick={() => input.current?.click()} dangerouslySetInnerHTML={{ __html: IC.plus }} /><input ref={input} type="file" multiple hidden onChange={event => {
    const files = [...(event.currentTarget.files ?? [])];
    event.currentTarget.value = "";
    if (files.length) void attach(files);
  }} /></>;
}
