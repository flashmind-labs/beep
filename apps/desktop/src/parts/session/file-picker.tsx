import { useRef } from "react";
import { Icon } from "../../ui/icons.tsx";
import { Button } from "../../ui/kit.tsx";

export function FilePicker({ attach, disabled = false }: { attach(files: File[]): unknown; disabled?: boolean }) {
  const input = useRef<HTMLInputElement>(null);

  // The tap leaves the message field focused: WebKit moves focus on mousedown even when
  // pointerdown was prevented, and the phone hides this button once the field loses focus.
  return <><Button look="quiet" title="Attach files" aria-label="Attach files" disabled={disabled} onMouseDown={event => event.preventDefault()} onClick={() => input.current?.click()}><Icon name="plus" /></Button><input ref={input} type="file" multiple hidden onChange={event => {
    const files = [...(event.currentTarget.files ?? [])];
    event.currentTarget.value = "";
    if (files.length) void attach(files);
  }} /></>;
}
