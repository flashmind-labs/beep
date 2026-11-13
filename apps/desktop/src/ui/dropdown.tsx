import type { ReactNode } from "react";
import { Button, Menu, MenuItem, MenuTrigger, Popover } from "react-aria-components";
import { useDialogRef } from "./parts.ts";
import { Icon } from "./icons.tsx";

/// A single choice, opened from a compact button in a toolbar.
export function Dropdown<K extends string>({ label, options, value, onChange, icon = null }: {
  label: string;
  options: readonly { id: K; label: string }[];
  value: K;
  onChange(value: K): void;
  icon?: ReactNode;
}) {
  const dialog = useDialogRef<HTMLDivElement>();
  const selected = options.find(option => option.id === value);
  const choose = (key: string | number) => {
    const option = options.find(option => option.id === key);
    if (option) onChange(option.id);
  };

  return <MenuTrigger>
    <Button className="dbtn quiet small dropdown-trigger" aria-label={label}>{icon}<span>{selected?.label ?? label}</span><Icon name="caretDown"/></Button>
    <Popover ref={dialog} className="dropdown-popover" placement="bottom end" offset={6}>
      <Menu className="dropdown-menu" aria-label={label} selectionMode="single" disallowEmptySelection selectedKeys={[value]} onAction={choose}>
        {options.map(option => <MenuItem className="dropdown-option" id={option.id} key={option.id} textValue={option.label}><span className="dropdown-check"><Icon name="check"/></span><span>{option.label}</span></MenuItem>)}
      </Menu>
    </Popover>
  </MenuTrigger>;
}
