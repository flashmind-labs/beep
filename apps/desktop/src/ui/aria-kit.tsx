import type { ReactNode } from "react";
import { Dialog, Modal, ModalOverlay, Tab, TabList, Tabs, ToggleButton, ToggleButtonGroup, type Key } from "react-aria-components";

// Kit controls on React Aria, kept apart from kit.tsx because loading React
// Aria sets up global listeners a bare test DOM cannot take. Segmented is used.
// Their DOM differs from the hand-written versions, so a view that switches
// needs these CSS changes first:
// - Segmented draws `div[role=radiogroup]` with `button[role=radio][aria-checked]`
//   where `.hv-seg` is `span[role=group]` with `button[aria-pressed]`; rules on
//   `[aria-pressed=true]` must also match `[data-selected]`, and its buttons
//   carry the class `react-aria-ToggleButton`.
// - TabStrip draws `div.react-aria-Tabs > div[role=tablist]` with
//   `div[role=tab]` where the pane tabs are `button[role=tab]` straight in their
//   strip; rules on `.agtab` buttons must not rely on the button element or the
//   parent, and the selected tab is `[data-selected]`. Each tab points
//   `aria-controls` at a React Aria `TabPanel`, so a switching view renders its
//   views in `TabPanel`s inside the same `Tabs`.
// - Sheet draws the scrim, a Modal `div` and a Dialog `section`, where
//   `openSheet` draws the scrim and a `div[role=dialog]`; give the Modal div
//   `display: contents` or move the sheet's layout rules onto it. React Aria
//   keeps its own overlay stack: Escape closes the top Sheet, but a Sheet and an
//   `openSheet` sheet open together do not see each other, since `openSheet`
//   takes Escape on the document in the capture phase. Focus goes back to what
//   had it when a Sheet closes, as with `openSheet`.

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

/// A row of tabs; the caller draws the selected tab's view.
export function TabStrip<K extends string>({ label, tabs, selected, onSelect, className }: {
  label: string; tabs: readonly { id: K; label: ReactNode; className?: string }[]; selected: K; onSelect(id: K): void; className?: string;
}) {
  const select = (key: Key) => {
    const picked = tabs.find(tab => tab.id === key);
    if (picked) onSelect(picked.id);
  };

  return <Tabs selectedKey={selected} onSelectionChange={select}>
    <TabList aria-label={label} className={className}>{tabs.map(tab => <Tab key={tab.id} id={tab.id} className={tab.className}>{tab.label}</Tab>)}</TabList>
  </Tabs>;
}

/// A sheet over the page: a click on the scrim or Escape closes it unless it
/// may not close, and focus goes back to what had it.
export function Sheet({ label, className, open, canClose = true, onClose, children }: {
  label: string; className: string; open: boolean; canClose?: boolean; onClose(): void; children: ReactNode;
}) {
  return <ModalOverlay className="am-scrim" isOpen={open} isDismissable={canClose} isKeyboardDismissDisabled={!canClose} onOpenChange={next => { if (!next) onClose(); }}>
    <Modal><Dialog className={className} aria-label={label}>{children}</Dialog></Modal>
  </ModalOverlay>;
}
