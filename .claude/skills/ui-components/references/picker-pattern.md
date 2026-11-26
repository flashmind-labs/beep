# Structured Glass Picker Reference

Canonical TSX and CSS pattern from `apps/desktop/src/settings/general-view.tsx`
and `apps/desktop/dist/native.css`.

## TSX Structure

```tsx
import { Button as AriaButton } from "react-aria-components/Button";
import { Header } from "react-aria-components/Header";
import { Menu, MenuItem, MenuSection, MenuTrigger } from "react-aria-components/Menu";
import { Popover } from "react-aria-components/Popover";
import { Separator } from "react-aria-components/Separator";
import { Icon } from "../ui/icons.tsx";
import { spoken } from "../ui/format.ts";
import { useDialogRef } from "../ui/parts.tsx";

export type PickerTagTone = "neutral" | "ready" | "dim";

export interface PickerItem<K extends string = string> {
  id: K;
  name: string;
  meta?: string;
  tag?: { text: string; tone?: PickerTagTone };
  disabled?: boolean;
}

export function StructuredPicker<K extends string>({
  id,
  fieldLabel,
  badge,
  accentBadge = false,
  selectedLabel,
  value,
  sections,
  disabled = false,
  onChange,
}: {
  id?: string;
  fieldLabel: string;
  badge?: string;
  accentBadge?: boolean;
  selectedLabel: string;
  value: K;
  sections: readonly { title?: string; items: readonly PickerItem<K>[] }[];
  disabled?: boolean;
  onChange(next: K): void;
}) {
  const dialog = useDialogRef<HTMLDivElement>();

  const renderItem = (item: PickerItem<K>) => {
    const tone = item.tag?.tone ?? "neutral";
    const tagCls = tone === "ready" ? "tag ready" : tone === "dim" ? "tag dim" : "tag";
    return (
      <MenuItem
        id={item.id}
        key={item.id}
        isDisabled={item.disabled}
        className="dict-opt"
        textValue={item.meta ? `${item.name} ${item.meta}` : item.name}
      >
        <span className="chk" aria-hidden="true"><Icon name="check" size={12} /></span>
        <span className="main">
          <span className="nm">{item.name}</span>
          {item.meta && <span className="meta">{item.meta}</span>}
        </span>
        {item.tag && <span className={tagCls}>{item.tag.text}</span>}
      </MenuItem>
    );
  };

  return (
    <MenuTrigger>
      <AriaButton
        id={id}
        className="dict-trigger"
        isDisabled={disabled}
        aria-label={spoken(fieldLabel, badge, selectedLabel)}
      >
        {badge && <span className={accentBadge ? "src local" : "src"}>{badge}</span>}
        <span>{selectedLabel}</span>
        <Icon name="caretDown" size={12} />
      </AriaButton>
      <Popover ref={dialog} className="dict-popover" placement="bottom start" offset={6}>
        <Menu
          className="dict-menu"
          aria-label={fieldLabel}
          selectionMode="single"
          disallowEmptySelection
          selectedKeys={[value]}
          onAction={key => onChange(key as K)}
        >
          {sections.map((section, idx) => (
            <MenuSection key={section.title ?? idx} className="dict-sec">
              {idx > 0 && <Separator className="dict-sep" />}
              {section.title && <Header className="dict-sec-hd">{section.title}</Header>}
              {section.items.map(renderItem)}
            </MenuSection>
          ))}
        </Menu>
      </Popover>
    </MenuTrigger>
  );
}
```

## Token-Pure CSS (`style.css` when shared across desktop + mobile, or `native.css` under `body:not(.mobile)`)

```css
.dict-trigger {
  height: var(--ctl-h);
  padding: 0 8px 0 9px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--ink);
  color: var(--fg);
  font: 500 12.5px/1 var(--body);
  display: inline-flex;
  align-items: center;
  gap: 7px;
  outline: 0;
  transition: border-color .12s, background .12s;
}
.dict-trigger:hover:not([data-disabled]) {
  border-color: var(--border-hi);
  background: var(--elev);
}
.dict-trigger[data-pressed],
.dict-trigger[aria-expanded="true"],
.dict-trigger[data-focus-visible] {
  border-color: var(--accent);
}
.dict-trigger[data-disabled] { opacity: .55; }
.dict-trigger .src {
  font: 500 10.5px/16px var(--mono);
  padding: 0 5px;
  border-radius: calc(var(--radius-sm) * .8);
  background: var(--ctl-fill);
  color: var(--muted);
}
.dict-trigger .src.local {
  color: color-mix(in srgb, var(--accent) 55%, var(--fg));
  background: color-mix(in srgb, var(--accent) 16%, transparent);
}
.dict-trigger > svg { flex: none; color: var(--muted); }

.dict-popover {
  position: relative;
  z-index: 60;
  width: 316px;
  max-width: calc(100vw - 24px);
  padding: 5px;
  border-radius: var(--radius);
  border: 1px solid color-mix(in srgb, var(--fg) 14%, var(--border));
  color: var(--fg);
  box-shadow: inset 0 1px 0 var(--lit-edge), 0 16px 40px color-mix(in srgb, var(--shade) 55%, transparent);
  font: 12.5px var(--body);
  overflow: hidden;
  outline: none;
}
.dict-popover::before {
  content: "";
  position: absolute;
  inset: 0;
  z-index: -1;
  background: color-mix(in srgb, var(--elev) max(84%, var(--glass-solid)), transparent);
  -webkit-backdrop-filter: var(--glass);
  backdrop-filter: var(--glass);
}

.dict-menu, .dict-sec { outline: none; }
.dict-sec-hd {
  padding: 7px 9px 4px 29px;
  font: 500 10px/1 var(--mono);
  letter-spacing: .08em;
  text-transform: uppercase;
  color: var(--dim);
}
.dict-sep { height: 1px; margin: 4px 5px; border: 0; background: var(--border); }

.dict-opt {
  padding: 6px 9px;
  border-radius: var(--radius-sm);
  display: grid;
  grid-template-columns: 14px minmax(0, 1fr) auto;
  align-items: center;
  column-gap: 6px;
  outline: none;
  cursor: default;
}
.dict-opt[data-hovered], .dict-opt[data-focused] {
  background: color-mix(in srgb, var(--fg) 8%, transparent);
}
.dict-opt[data-selected] {
  background: color-mix(in srgb, var(--accent) 14%, transparent);
}
.dict-opt .chk {
  width: 14px;
  height: 14px;
  display: grid;
  place-items: center;
  color: var(--accent);
  visibility: hidden;
}
.dict-opt[data-selected] .chk { visibility: visible; }
.dict-opt .main { min-width: 0; display: flex; align-items: baseline; gap: 6px; }
.dict-opt .nm { font-size: 12.5px; font-weight: 500; color: var(--fg); white-space: nowrap; }
.dict-opt .meta {
  font-size: 11.5px;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dict-opt .tag {
  font: 500 10.5px/16px var(--mono);
  padding: 0 6px;
  border-radius: calc(var(--radius-sm) * .8);
  background: color-mix(in srgb, var(--fg) 6%, transparent);
  color: var(--muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.dict-opt .tag.ready {
  color: var(--ok);
  background: color-mix(in srgb, var(--ok) 12%, transparent);
}
.dict-opt .tag.dim { background: transparent; color: var(--dim); padding-right: 2px; }
```
