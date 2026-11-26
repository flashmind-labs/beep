// Shared command-center building blocks. Each one draws the exact markup the
// views wrote by hand (`cc-sec`, `cc-panel cc-empty`, `cc-line`, `chip`,
// `dspin`), so a view can switch to it without a visible change.
import { cloneElement, isValidElement, lazy, Suspense, useEffect, useLayoutEffect, useMemo, useRef, useState, type ButtonHTMLAttributes, type CSSProperties, type HTMLAttributes, type KeyboardEvent, type ReactNode, type Ref, type SyntheticEvent } from "react";
import parse from "html-react-parser";
import { copy } from "./clipboard.ts";
import { Icon, type IconName } from "./icons.tsx";
import type { DropdownProps } from "./dropdown.tsx";

const LoadedDropdown = lazy(() => import("./dropdown.tsx").then(({ Dropdown }) => ({ default: Dropdown<string> })));

/// `Dropdown`, loaded on first draw, so modules that only import a view stay free of the menu library.
export function Picker<K extends string>({ onChange, ...props }: DropdownProps<K>) {
  const choose = (id: string) => {
    const option = props.options.find(option => option.id === id);
    if (option) onChange(option.id);
  };

  return <Suspense fallback={null}><LoadedDropdown {...props} onChange={choose} /></Suspense>;
}

/// Sanitized HTML as React elements keyed by place, so a block that stays keeps its
/// element: the reader's selection, and a diagram drawn next to it, survive an update.
export function useHtml(html: string) {
  return useMemo(() => {
    const nodes = parse(html);
    return (Array.isArray(nodes) ? nodes : [nodes]).map((node, i) => isValidElement(node) ? cloneElement(node, { key: i }) : node);
  }, [html]);
}

/// A section: its head (title, count, then `action`) over a panel of `children`.
export function Section({ id, title, count = 0, action, children }: { id?: string; title: ReactNode; count?: number | string; action?: ReactNode; children: ReactNode }) {
  return <div id={id}><SectionHead title={title} count={count}>{action}</SectionHead><div className="cc-panel">{children}</div></div>;
}

/// A section's head alone: its title and count, then notes or buttons.
export function SectionHead({ title, count = 0, children }: { title: ReactNode; count?: number | string; children?: ReactNode }) {
  return <div className="cc-sec"><h4>{title}{!!count && <span className="n">{count}</span>}</h4>{children}</div>;
}

/// The title row of a sheet, with the key that closes it. `children` go inside the title.
export function SheetHead({ title, children }: { title: ReactNode; children?: ReactNode }) {
  return <div className="am-head"><b>{title}{children}</b><kbd>esc</kbd></div>;
}

/// Grey bars while a list loads, one per width in percent.
export function SkeletonRows({ widths }: { widths: readonly number[] }) {
  return <div className="dskel-rows">{widths.map(width => <div className="dskel" key={width} style={{ width: `${width}%` }} />)}</div>;
}

/// One row of a section: `lead` (a harness or icon), a title over one muted
/// line, then `right`. A click anywhere opens it. `current` marks the row in hand.
export function LineRow({ lead, title, sub, right, onOpen, onContext, dim = false, current = false, rowTitle, textTitle, dataKey, events }: {
  lead: ReactNode; title: string; sub: ReactNode; right?: ReactNode; onOpen(): void; onContext?(event: MouseEvent): void;
  events?: Pick<HTMLAttributes<HTMLDivElement>, "onClickCapture" | "onAuxClick" | "onMouseEnter" | "onMouseLeave" | "onContextMenu">;
  dim?: boolean; current?: boolean; rowTitle?: string; textTitle?: string; dataKey?: string;
}) {
  return <div data-key={dataKey} className={`cc-row cc-line${lead ? "" : " bare"}`} aria-current={current || undefined} title={rowTitle} style={dim ? { opacity: .55 } : undefined} onClick={onOpen} onContextMenu={onContext ? event => onContext(event.nativeEvent) : undefined} {...events}>
    {lead}<button type="button" className="t" title={textTitle ?? title}><b>{title}</b><span>{sub}</span></button><span className="r">{right}</span>
  </div>;
}

/// The fold a list keeps its ended rows under, closed until opened.
export function EndedFold({ count, open, onToggle }: { count: number; open: boolean; onToggle(): void }) {
  return <button type="button" className="dagfold" aria-expanded={open} onClick={onToggle}><span className="chev" aria-hidden>{open ? "▾" : "▸"}</span>Ended<span className="n">{count}</span></button>;
}

/// A panel with nothing to list: `title` in bold, then `text` (red when it is
/// an `error`), after a spinner while it `spin`s.
export function EmptyPanel({ title = null, text = null, error = false, spin = false }: { title?: ReactNode; text?: ReactNode; error?: boolean; spin?: boolean }) {
  return <div className="cc-panel cc-empty">{spin && <Spinner />}{title !== null && <b>{title}</b>}{text !== null && <span className={error ? "cc-err" : undefined}>{text}</span>}</div>;
}

/// A status chip: `kind` colors it (`run`, `wait`, `bad`, `done`, ...), with
/// a running dot first while it `spin`s.
export function Chip({ kind, text, spin = false, title }: { kind: string; text: ReactNode; spin?: boolean; title?: string }) {
  return <span className={kind ? `chip ${kind}` : "chip"} title={title}>{spin && <Spinner dot />}{text}</span>;
}

/// A chip that toggles a choice on and off, with `icon` before its label.
export function ChipToggle({ label, selected, onToggle, icon = null }: { label: string; selected: boolean; onToggle(): void; icon?: IconName | null }) {
  return <button type="button" className="dbtn dchip" aria-pressed={selected} onClick={onToggle}>{icon && <Icon name={icon} size={11} />}{label}</button>;
}

/// Turns off browser and macOS autofill, autocorrect, auto-capitalization and spellcheck.
export const NO_AUTO = { autoComplete: "off", autoCorrect: "off", autoCapitalize: "off", spellCheck: false } as const;

/// How much a button stands out. A dialog or form has one `primary`, its main action;
/// `secondary` is bordered, `quiet` is text until hovered, `plain` keeps a view's own style.
export type ButtonLook = "primary" | "secondary" | "quiet" | "plain";
const LOOK_CLASS: Record<ButtonLook, string> = { primary: " pri", secondary: "", quiet: " quiet", plain: "" };

type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "type"> & {
  look?: ButtonLook; danger?: boolean; small?: boolean; type?: "button" | "submit"; ref?: Ref<HTMLButtonElement>;
};

/// The app's button (`dbtn`). `danger` colors it for an action that removes or stops
/// something; `className` adds a view's own layout class.
export function Button({ look = "secondary", danger = false, small = false, type = "button", className, ...rest }: ButtonProps) {
  const name = `${look === "plain" ? "" : "dbtn"}${LOOK_CLASS[look]}${danger ? " warn" : ""}${small ? " small" : ""}${className ? ` ${className}` : ""}`;
  return <button type={type} className={name.trim()} {...rest} />;
}

/// Keeps a click or key on a control from reaching the row around it.
const contain = (event: SyntheticEvent) => event.stopPropagation();

/// The app's checkbox (`dcheck`). `mixed` draws the half-checked state for a
/// choice only partly made. `contained` keeps clicks from the row it sits in.
export function Checkbox({ checked, change, mixed = false, disabled = false, label, tabIndex, contained = false }: {
  checked: boolean; change(checked: boolean): void; mixed?: boolean; disabled?: boolean; label?: string; tabIndex?: number; contained?: boolean;
}) {
  const box = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { if (box.current) box.current.indeterminate = mixed; }, [mixed]);
  return <input ref={box} type="checkbox" className="dcheck" checked={checked} disabled={disabled} aria-label={label} tabIndex={tabIndex}
    onClick={contained ? contain : undefined} onKeyDown={contained ? contain : undefined} onChange={event => change(event.currentTarget.checked)} />;
}

/// The app's on/off switch (`dswitch`). `contained` keeps clicks and keys from the row it sits in.
export function Switch({ on, flip, label, disabled = false, id, contained = false }: {
  on: boolean; flip(): void; label: string; disabled?: boolean; id?: string; contained?: boolean;
}) {
  return <button type="button" className="dswitch" role="switch" id={id} aria-checked={on} aria-label={label} disabled={disabled}
    onClick={event => { if (contained) contain(event); flip(); }} onKeyDown={contained ? contain : undefined} />;
}

/// The thin bar along the top of a message field while something it waits on copies, such as an
/// upload or a move, filled to `pct`.
export function ProgressBar({ pct, label }: { pct: number; label: string }) {
  return <div className="agupbar" role="progressbar" aria-label={label} aria-valuenow={pct} title={`${label}: ${pct}%`}><span style={{ width: `${pct}%` }} /></div>;
}

/// A progress indicator: the 4x4 Neural Spark dot-matrix (`dspin`, or with `dot`
/// the smaller `drun` for background work). A `label` makes it an image screen
/// readers announce. It starts at the page clock's point in the cycle so one
/// drawn again continues where the last left off instead of starting over.
export function Spinner({ dot = false, className, label, title }: { dot?: boolean; className?: string; label?: string; title?: string }) {
  const [style] = useState(matrixPhase);
  return <span className={`${dot ? "drun" : "dspin"}${className ? ` ${className}` : ""}`} style={style} role={label ? "img" : undefined} aria-label={label} title={title}><DotMatrix size="sm" synced /></span>;
}

type MatrixStyle = CSSProperties & { "--mx-t"?: string; "--p"?: string };
interface MatrixCell { key: number; x: number; y: number; cls: string; delay: MatrixStyle | undefined }

/// One cycle of `dspark` in style.css.
const MATRIX_MS = 2400;

/// How far into the cycle every dot-matrix is now, as a negative offset shared by its cells.
const matrixPhase = (): MatrixStyle => ({ "--mx-t": `${-Math.round(performance.now() % MATRIX_MS)}ms` });

/// Builds a grid of rounded dot-matrix cells where two braided spark paths fire
/// with phosphor decay and adjacent cells catch a softer halo.
function buildMatrix(grid: number, paths: readonly (readonly (readonly [number, number])[])[], offset: number): readonly MatrixCell[] {
  const stepOf = new Map<number, number>();
  paths.forEach((path, pathIndex) => {
    path.forEach(([x, y], step) => {
      const idx = y * grid + x;
      if (!stepOf.has(idx)) stepOf.set(idx, ((step + pathIndex * offset) % path.length) / path.length);
    });
  });

  const cells: MatrixCell[] = [];
  for (let y = 0; y < grid; y++) {
    for (let x = 0; x < grid; x++) {
      const idx = y * grid + x;
      const direct = stepOf.get(idx);
      if (direct !== undefined) {
        const lit = direct < 0.28;
        cells.push({ key: idx, x: x * 4, y: y * 4, cls: lit ? "hi lit" : "hi", delay: { "--p": `${-Math.round(direct * MATRIX_MS)}ms` } });
        continue;
      }

      let near: number | undefined;
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]] as const) {
        const nx = x + dx, ny = y + dy;
        if (nx >= 0 && nx < grid && ny >= 0 && ny < grid) {
          const hit = stepOf.get(ny * grid + nx);
          if (hit !== undefined && (near === undefined || hit < near)) near = hit;
        }
      }
      if (near !== undefined) {
        const phase = (near + 0.04) % 1;
        cells.push({ key: idx, x: x * 4, y: y * 4, cls: phase < 0.25 ? "lo mid" : "lo", delay: { "--p": `${-Math.round(phase * MATRIX_MS)}ms` } });
      } else {
        cells.push({ key: idx, x: x * 4, y: y * 4, cls: "off", delay: undefined });
      }
    }
  }
  return cells;
}

// Two counter-weaving spark walks on a 7x7 grid (for inline live activity).
const MATRIX_7 = buildMatrix(7, [
  [[1, 1], [1, 2], [2, 2], [2, 3], [3, 3], [3, 4], [4, 4], [4, 5], [5, 4], [5, 3], [4, 2], [3, 1]],
  [[5, 1], [4, 1], [3, 2], [2, 3], [1, 4], [1, 5], [2, 5], [3, 5], [4, 4], [5, 3], [5, 2], [4, 2]],
], 5);

// Compact braided spark walk on a 4x4 grid (for Spinners).
const MATRIX_4 = buildMatrix(4, [
  [[0, 1], [1, 1], [1, 2], [2, 2], [2, 1], [3, 1], [2, 0], [1, 0]],
  [[3, 2], [2, 2], [2, 3], [1, 3], [1, 2], [0, 2], [1, 1], [2, 1]],
], 4);

/// Inline Neural Spark dot-matrix: `md` (7x7) for live thinking/activity lines,
/// or `sm` (4x4) inside `Spinner`. Pure SVG + CSS keyframes so hidden windows
/// and Reduce Motion stop it with no timers.
export function DotMatrix({ size = "md", synced = false }: { size?: "sm" | "md"; synced?: boolean }) {
  const [style] = useState(() => synced ? undefined : matrixPhase());
  const cells = size === "sm" ? MATRIX_4 : MATRIX_7;
  const box = size === "sm" ? "0 0 15 15" : "0 0 27 27";
  return <svg className={`dmx ${size}`} viewBox={box} style={style} aria-hidden="true">
    {cells.map(cell => <rect key={cell.key} x={cell.x} y={cell.y} width="3" height="3" rx="0.9" className={cell.cls} style={cell.delay} />)}
  </svg>;
}

/// Enter or Space on a focusable row that is not itself a button runs `run`,
/// as a click on it would. Keys from controls inside the row pass by.
export function onRowKey(run: () => void) {
  return (event: KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget || (event.key !== "Enter" && event.key !== " ")) return;
    event.preventDefault();
    run();
  };
}

/// How many of `total` rows to draw: `initial` first, then `step` more each time `end`, a marker
/// after the last drawn row, comes within 800px of its scroll area. `first` wraps the drawn rows;
/// once `rowOf` finds the first row in it, `height` is one row's measured height, for sizing the
/// marker as the rows not drawn yet.
export function useLazyCount<F extends HTMLElement>(total: number, initial: number | (() => number), step: number, rowOf: (first: F) => Element | null = first => first) {
  const [count, setCount] = useState(initial), [height, setHeight] = useState(0), end = useRef<HTMLDivElement>(null), first = useRef<F>(null);
  useEffect(() => {
    const el = end.current;
    if (!el || typeof IntersectionObserver === "undefined") { if (typeof IntersectionObserver === "undefined") setCount(total); return; }
    let root: Element | null = el.parentElement;
    while (root && !/(auto|scroll)/.test(getComputedStyle(root).overflowY)) root = root.parentElement;
    const observer = new IntersectionObserver(entries => {
      const row = !height && first.current ? rowOf(first.current) : null;
      if (row) { const measured = (el.getBoundingClientRect().top - row.getBoundingClientRect().top) / Math.min(count, total); if (measured > 0) setHeight(measured); }
      if (entries.some(entry => entry.isIntersecting)) setCount(value => Math.min(total, value + step));
    }, { root, rootMargin: "800px 0px" });
    observer.observe(el);
    return () => observer.disconnect();
  }, [count, height, total]);
  return { count, height, end, first };
}

/// A button that copies `text` and reads "Copied" for a moment, then `label` again.
/// With `icon` it shows the copy icon, then a check.
export function CopyButton({ text, label = "Copy", look = "secondary", small = false, icon = false, id, className }: { text: string; label?: ReactNode; look?: ButtonLook; small?: boolean; icon?: boolean; id?: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(timer);
  }, [copied]);

  const onClick = () => { void copy(text).then(() => setCopied(true), error => console.warn("copying:", error)); };
  if (icon) return <Button look={look} small={small} id={id} className={className} aria-label={copied ? "Copied" : "Copy"} title={copied ? "Copied" : "Copy"} onClick={onClick}><Icon name={copied ? "check" : "copy"} /></Button>;

  return <Button look={look} small={small} id={id} className={className} onClick={onClick}>{copied ? "Copied" : label}</Button>;
}
