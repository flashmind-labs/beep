---
name: ui-components
description: >-
  Use when building, restyling, or auditing Beep desktop or mobile UI controls
  in apps/desktop, especially replacing native <select> elements, building
  dropdowns, menus, popovers, settings rows, source badges, status chips,
  buttons, switches, or inline download and progress states.
---

# Beep UI Components

Standardize controls in `apps/desktop` around Beep's dark chrome, skin tokens,
and React Aria primitives. The canonical reference implementation is
`DictationModelPicker` and `DictationPreference` in
`apps/desktop/src/settings/general-view.tsx` paired with `.dict-*` in
`apps/desktop/dist/native.css`.

For copy-ready TSX and CSS templates, read
[references/picker-pattern.md](./references/picker-pattern.md).

## 1. Core Rules

- **Never use native `<select>` for app chrome.** Browser `<select>` menus draw
  OS chrome, ignore skins and glass, and cannot show secondary labels or status
  pills. Use `Dropdown` (`apps/desktop/src/ui/dropdown.tsx`) for flat single-label
  lists, or the structured React Aria picker pattern below when items have
  sections, secondary metadata, or live status tags. When two views need the
  structured picker, promote it into `apps/desktop/src/ui/dropdown.tsx`.
- **Register every floating layer with `useDialogRef`.** Attach
  `ref={useDialogRef<HTMLDivElement>()}` (`apps/desktop/src/ui/parts.tsx`) to
  every React Aria `<Popover>` so `Escape` closes only the open menu, never the
  parent Settings sheet or modal (`sheet.ts` and `openSheet` check `dialogAbove`
  and `dialogOpen`).
- **Put backdrop blur on `::before`, never on the popover element itself.**
  Filtering the container breaks fixed descendants and nested layers. Use
  `color-mix(in srgb, var(--elev) max(84%, var(--glass-solid)), transparent)`
  with `backdrop-filter: var(--glass)` on `::before` so flat skins
  (`--glass: none; --glass-solid: 100%`) also render opaque cleanly.
- **Skin tokens only, zero hardcoded hex colors.** Derive tints with
  `color-mix(in srgb, ...)` from tokens in `apps/desktop/dist/skins.css`
  (`--ink`, `--surface`, `--elev`, `--deep`, `--border`, `--border-hi`, `--fg`,
  `--muted`, `--dim`, `--accent`, `--on-accent`, `--ok`, `--warn`, `--err`,
  `--ctl-h`, `--ctl-fill`, `--ctl-fill-hi`, `--lit-edge`, `--radius`,
  `--radius-sm`, `--body`, `--mono`, `--glass`, `--glass-solid`, `--shade`).
- **Scope layout container rules to direct children (`>`), never bare tag descendants.**
  Writing `.dtoggle span { flex: 1; display: grid }` leaks layout properties
  into nested badges, `.dgchip` pills, and button labels inside `extra` or
  `children`. Target `.dtoggle > span:first-child` or a dedicated slot class,
  and put `flex: none; white-space: nowrap` on compact triggers and pills.
- **Promote shared controls to `apps/desktop/src/ui/kit.tsx`.** Reuse `Button`,
  `Switch`, `Checkbox`, `Chip`, `ChipToggle`, `Spinner`, `ProgressBar`,
  `Section`, `SectionHead`, `SheetHead`, `LineRow`, `EmptyPanel`, and
  `CopyButton` instead of hand-rolling raw buttons or switches.

## 2. Structured Picker Anatomy

1. **Trigger (`react-aria-components/Button`, `height: var(--ctl-h)`):**
   - Fill `var(--ink)` on `var(--surface)` rows, `1px solid var(--border)`,
     hover `var(--elev)` + `var(--border-hi)`, active/expanded/focus-visible
     `border-color: var(--accent)`.
   - Optional leading **source badge** (`.src`: `500 10.5px/16px var(--mono)`,
     `background: var(--ctl-fill)`, `color: var(--muted)`; `.src.local` uses
     `color-mix` with `var(--accent)` when highlighting on-device capability).
   - Primary item name (`500 12.5px/1 var(--body)`, `color: var(--fg)`).
   - Trailing `<Icon name="caretDown" size={12} />` in `var(--muted)`.
   - Build `aria-label` with `spoken(fieldLabel, badge, itemName)` from
     `apps/desktop/src/ui/format.ts`.
2. **Popover & Menu (`MenuTrigger` -> `Popover` -> `Menu`):**
   - Pass `selectionMode="single"`, `disallowEmptySelection`, and
     `selectedKeys={[currentKey]}` on `<Menu>`. Use a non-empty string key such
     as `"auto"` for null/default choices and map it back in `onAction`.
   - Separate groups with `<Separator>` (`1px` `var(--border)`) and
     `<MenuSection>` + `<Header>` (`500 10px/1 var(--mono)`, uppercase,
     `letter-spacing: .08em`, `color: var(--dim)`, `padding-left: 29px` to align
     with item text past the check column).
3. **3-column option row (`grid-template-columns: 14px minmax(0, 1fr) auto`):**
   - Column 1 (`14px`): `<Icon name="check" size={12} />` in `var(--accent)`,
     `visibility: hidden` until `[data-selected]`.
   - Column 2 (`minmax(0, 1fr)`): baseline flex row with primary name (`12.5px`,
     weight `500`, `var(--fg)`) + muted detail (`11.5px`, `var(--muted)`,
     `text-overflow: ellipsis`). Set `textValue` on `<MenuItem>` for typeahead.
   - Column 3 (`auto`): right-aligned mono status tag (`500 10.5px/16px var(--mono)`,
     `font-variant-numeric: tabular-nums`):
     - Neutral metadata (`731 MB`, `…4a9f`, `43%`): `color-mix(in srgb, var(--fg) 6%, transparent)` fill, `var(--muted)` text.
     - Ready state (`On device`, `Online`): `color-mix(in srgb, var(--ok) 12%, transparent)` fill, `var(--ok)` text.
     - Unset or passive (`No key`, `Auto`, `Offline`): transparent fill, `var(--dim)` text.

## 3. Companion Row Controls & Progressive States

Pair pickers in `SettingRow` (`apps/desktop/src/settings/overview-view.tsx`) or
cards with state-driven inline actions inside a wrapping flex row (`gap: 8px`):

- **Needs one-time primary action (e.g. local download):**
  `<Button look="primary" small>Download ({bytes(total)})</Button>` with no
  irrelevant credential controls shown.
- **Needs setup elsewhere (e.g. missing API key):**
  `<Button look="secondary" small>Add API key…</Button>`.
- **In-progress determinate transfer:**
  `<span className="fs-pct"><span className="fs-bar"><i style={{ width: `${pct}%` }} /></span>{pct}%</span>`
  (`tabular-nums` in `--mono`, `var(--accent)` bar fill) beside
  `<Button look="quiet" small>Cancel</Button>`.
- **Ready / configured:**
  `<span className="dgchip ready">On device · 731 MB</span>` (`.dgchip.ready` in
  `style.css`) or `<span className="dgchip">Mistral key …9c12</span>` plus quiet
  secondary action `<Button look="quiet" small>Credentials…</Button>`.
- **Subtitle (`note`):** state what happens next in one plain sentence fragment
  without repeating the trigger's badge and model name. Never use em-dashes or
  Unicode arrows (`->` or words only).

## 4. CSS Placement & Migration Targets

- **CSS files (`apps/desktop/dist/`):**
  - `style.css`, `center.css`, `sheets.css`: shared unscoped component rules;
    hide desktop-only elements by default.
  - `native.css`: desktop rules scoped under `body:not(.mobile)`.
  - `mobile.css`: phone overrides (`body.mobile`).
- **Remaining native `<select>` elements to upgrade when touched:**
  - `apps/desktop/src/settings/credentials-view.tsx` (`#credProvider`)
  - `apps/desktop/src/settings/providers-view.tsx` (`.hv-sel`)
  - `apps/desktop/src/machines/mcp-servers-view.tsx` (`Transport`, `Default tool approval`)
  - `apps/desktop/src/agentpane/device-view.tsx` (`.agdevpick`)
  - `apps/desktop/src/agentpane/model-controls.tsx` (`Clip`)

## 5. Verification

1. Typecheck and unit tests: `npm --prefix apps/desktop run check`
2. Bundle before browser tests: `npm --prefix apps/desktop run build`
3. Playwright spec (`apps/desktop/tests/browser/`): assert keyboard `Escape`
   closes the open `<Menu>` while keeping any parent `dialog` open, and test
   each progressive state transition.
4. Text layout snapshot: run `npx playwright test tests/browser/ui-baseline.spec.ts --project=macOS --update-snapshots`,
   inspect `git diff` on `.txt` snapshots, and delete any generated `.png` files
   before staging.
