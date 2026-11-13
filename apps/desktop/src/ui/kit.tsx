// Shared command-center building blocks. Each one draws the exact markup the
// views wrote by hand (`cc-sec`, `cc-panel cc-empty`, `cc-line`, `chip`,
// `dspin`), so a view can switch to it without a visible change.
import { useEffect, useLayoutEffect, useRef, useState, type ButtonHTMLAttributes, type KeyboardEvent, type ReactNode, type Ref, type SyntheticEvent } from "react";
import { copy } from "./parts.ts";

/// A section: its head (title, count, then `action`) over a panel of `children`.
export function Section({ id, title, count = 0, action, children }: { id?: string; title: ReactNode; count?: number | string; action?: ReactNode; children: ReactNode }) {
  return <div id={id}><SectionHead title={title} count={count}>{action}</SectionHead><div className="cc-panel">{children}</div></div>;
}

/// A section's head alone: its title and count, then notes or buttons.
export function SectionHead({ title, count = 0, children }: { title: ReactNode; count?: number | string; children?: ReactNode }) {
  return <div className="cc-sec"><h4>{title}{!!count && <span className="n">{count}</span>}</h4>{children}</div>;
}

/// One row of a section: `lead` (a harness or icon), a title over one muted
/// line, then `right`. A click anywhere opens it.
export function LineRow({ lead, title, sub, right, onOpen, onContext, dim = false, rowTitle, textTitle, dataKey }: {
  lead: ReactNode; title: string; sub: ReactNode; right?: ReactNode; onOpen(): void; onContext?(event: MouseEvent): void;
  dim?: boolean; rowTitle?: string; textTitle?: string; dataKey?: string;
}) {
  return <div data-key={dataKey} className={`cc-row cc-line${lead ? "" : " bare"}`} title={rowTitle} style={dim ? { opacity: .55 } : undefined} onClick={onOpen} onContextMenu={onContext ? event => onContext(event.nativeEvent) : undefined}>
    {lead}<button type="button" className="t" title={textTitle ?? title}><b>{title}</b><span>{sub}</span></button><span className="r">{right}</span>
  </div>;
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

/// A chip that toggles a choice on and off.
export function ChipToggle({ label, selected, onToggle }: { label: string; selected: boolean; onToggle(): void }) {
  return <button type="button" className="dbtn dchip" aria-pressed={selected} onClick={onToggle}>{label}</button>;
}

/// How much a button stands out. A dialog or form has one `primary`, its main action;
/// `secondary` is bordered, `quiet` is text until hovered.
export type ButtonLook = "primary" | "secondary" | "quiet";
const LOOK_CLASS: Record<ButtonLook, string> = { primary: " pri", secondary: "", quiet: " quiet" };

type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "type"> & {
  look?: ButtonLook; danger?: boolean; small?: boolean; type?: "button" | "submit"; ref?: Ref<HTMLButtonElement>;
};

/// The app's button (`dbtn`). `danger` colors it for an action that removes or stops
/// something; `className` adds a view's own layout class.
export function Button({ look = "secondary", danger = false, small = false, type = "button", className, ...rest }: ButtonProps) {
  const name = `dbtn${LOOK_CLASS[look]}${danger ? " warn" : ""}${small ? " small" : ""}${className ? ` ${className}` : ""}`;
  return <button type={type} className={name} {...rest} />;
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

/// A spinner: the ring (`dspin`), or with `dot` the still running dot
/// (`drun`) for work that goes on by itself. A `label` makes it an image
/// screen readers announce. A ring starts at the page clock's point in the
/// turn, so one drawn again goes on where the last left off.
export function Spinner({ dot = false, className, label, title }: { dot?: boolean; className?: string; label?: string; title?: string }) {
  const [style] = useState(spinPhase);
  return <span className={`${dot ? "drun" : "dspin"}${className ? ` ${className}` : ""}`} style={dot ? undefined : style} role={label ? "img" : undefined} aria-label={label} title={title} />;
}

/// One turn of `spin` in style.css.
const SPIN_MS = 800;

/// How far into the turn every spinner is now, as a delay that starts a new one there.
const spinPhase = () => ({ animationDelay: `${-Math.round(performance.now() % SPIN_MS)}ms` });

/// Enter or Space on a focusable row that is not itself a button runs `run`,
/// as a click on it would. Keys from controls inside the row pass by.
export function onRowKey(run: () => void) {
  return (event: KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget || (event.key !== "Enter" && event.key !== " ")) return;
    event.preventDefault();
    run();
  };
}

/// A button that copies `text` and reads "Copied" for a moment, then `label` again.
export function CopyButton({ text, label = "Copy", look = "secondary", small = false, id }: { text: string; label?: ReactNode; look?: ButtonLook; small?: boolean; id?: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(timer);
  }, [copied]);

  return <Button look={look} small={small} id={id} onClick={() => { void copy(text).then(() => setCopied(true), error => console.warn("copying:", error)); }}>{copied ? "Copied" : label}</Button>;
}
