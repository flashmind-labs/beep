import type { ReactNode } from "react";
import { Button } from "react-aria-components/Button";
import { Header } from "react-aria-components/Header";
import { Menu, MenuItem, MenuSection, MenuTrigger } from "react-aria-components/Menu";
import { Popover } from "react-aria-components/Popover";
import { useDialogRef } from "./parts.tsx";
import { Icon } from "./icons.tsx";
import { spoken } from "./format.ts";

/// One choice. Options sharing a `section` are listed under it; `tag` is a short status on the right.
interface DropdownOption<K extends string> { id: K; label: string; section?: string; tag?: string; disabled?: boolean }

/// A single choice, opened from a compact toolbar button or, with `field`, a form field.
export interface DropdownProps<K extends string> {
  label: string;
  options: readonly DropdownOption<K>[];
  value: K | null;
  onChange(value: K): void;
  icon?: ReactNode;
  placeholder?: string;
  field?: boolean;
  disabled?: boolean;
  className?: string;
}

export function Dropdown<K extends string>({ label, options, value, onChange, icon = null, placeholder, field = false, disabled = false, className }: DropdownProps<K>) {
  const dialog = useDialogRef<HTMLDivElement>();
  const selected = options.find(option => option.id === value);
  const shown = selected?.label ?? placeholder ?? label;
  const choose = (key: string | number) => {
    const option = options.find(option => option.id === key);
    if (option) onChange(option.id);
  };
  const item = (option: DropdownOption<K>) => <MenuItem className="dropdown-option" id={option.id} key={option.id} textValue={option.label} isDisabled={option.disabled}>
    <span className="dropdown-check"><Icon name="check"/></span><span className="dropdown-label">{option.label}</span>{option.tag && <span className="dropdown-tag">{option.tag}</span>}
  </MenuItem>;
  const sections = options.reduce<{ name: string; options: DropdownOption<K>[] }[]>((all, option) => {
    const last = all.at(-1), name = option.section ?? "";
    if (last && last.name === name) last.options.push(option);
    else all.push({ name, options: [option] });
    return all;
  }, []);

  return <MenuTrigger>
    <Button className={`dropdown-trigger ${field ? "field" : "dbtn quiet small"}${className ? ` ${className}` : ""}`} isDisabled={disabled} aria-label={spoken(label, selected?.label)}>{icon}<span>{shown}</span><Icon name="caretDown"/></Button>
    <Popover ref={dialog} className="dropdown-popover" placement={field ? "bottom start" : "bottom end"} offset={6}>
      <Menu className="dropdown-menu" aria-label={label} selectionMode="single" disallowEmptySelection selectedKeys={value === null ? [] : [value]} onAction={choose}>
        {sections.some(section => section.name)
          ? sections.map(section => <MenuSection key={section.name} className="dropdown-section"><Header className="dropdown-head">{section.name}</Header>{section.options.map(item)}</MenuSection>)
          : options.map(item)}
      </Menu>
    </Popover>
  </MenuTrigger>;
}
