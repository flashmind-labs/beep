import type { Key } from "react-aria-components";
import { ToggleButton } from "react-aria-components/ToggleButton";
import { ToggleButtonGroup } from "react-aria-components/ToggleButtonGroup";

// Kit controls on React Aria, kept apart from kit.tsx because loading React
// Aria sets up global listeners a bare test DOM cannot take. Segmented draws
// `div[role=radiogroup]` with `button[role=radio][aria-checked]` where `.hv-seg`
// is `span[role=group]` with `button[aria-pressed]`; rules on
// `[aria-pressed=true]` must also match `[data-selected]`, and its buttons
// carry the class `react-aria-ToggleButton`.

/// One choice of a few, as a row of buttons.
export function Segmented<K extends string>({ label, options, value, onChange, className = "hv-seg" }: {
  label: string; options: readonly { id: K; label: string; disabled?: boolean }[]; value: K | null; onChange(id: K): void; className?: string;
}) {
  const change = (keys: Set<Key>) => {
    const picked = options.find(option => keys.has(option.id));
    if (picked) onChange(picked.id);
  };

  return <ToggleButtonGroup className={className} aria-label={label} selectionMode="single" disallowEmptySelection selectedKeys={value === null ? [] : [value]} onSelectionChange={change}>
    {options.map(option => <ToggleButton key={option.id} id={option.id} isDisabled={option.disabled}>{option.label}</ToggleButton>)}
  </ToggleButtonGroup>;
}
