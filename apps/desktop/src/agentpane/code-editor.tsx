// CodeMirror for a text tab being edited; loaded in its own chunk on the first edit.
import { indentWithTab } from "@codemirror/commands";
import { LanguageDescription, syntaxHighlighting, toggleFold } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { openSearchPanel, search, searchKeymap, searchPanelOpen } from "@codemirror/search";
import { Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  drawSelection,
  EditorView,
  keymap,
  lineNumbers,
  ViewPlugin,
} from "@codemirror/view";
import { getCM, Vim, vim } from "@replit/codemirror-vim";
import { basicSetup } from "codemirror";
import { useLayoutEffect, useRef } from "react";
import { primaryModifier } from "../ui/platform.ts";
import { tokens } from "../ui/syntax.ts";
import { wordSpanAt } from "./lsp-state.ts";
import type { FileEdit, TextPlace } from "./pane-state.ts";

export interface DefinitionRequest {
  line: number;
  column: number;
  text: string;
  clientX: number;
  clientY: number;
}

export default function CodeEditor({ edit, rel, changed, save, vimKeys, findRequest, closeSearch, goToDefinition }: {
  edit: FileEdit;
  rel: string;
  changed: () => void;
  save: () => void;
  vimKeys: boolean;
  findRequest: number;
  closeSearch: () => void;
  goToDefinition: (request: DefinitionRequest) => void;
}) {
  const host = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const parent = host.current;
    if (!parent) return;

    const state = edit.state ?? EditorState.create({
      doc: edit.base,
      extensions: setup(edit.base, changed, save, closeSearch),
    });
    const view = new EditorView({ parent, state });
    edit.view = view;
    definitionHandlers.set(view, goToDefinition);
    view.focus();

    // The language's parser loads on its own, after the text shows.
    let live = true;
    void LanguageDescription.matchFilename(languages, rel)?.load().then(support => {
      if (live) view.dispatch({ effects: language.reconfigure(support) });
    });

    return () => {
      live = false;
      definitionHandlers.delete(view);
      // A reload let go of this editor already, and its text with it.
      if (edit.view === view) { edit.state = view.state; edit.view = null; }
      view.destroy();
    };
    // The callbacks act on the tab whose edit this is, so the first ones do.
  }, [edit, rel]);

  if (edit.view) definitionHandlers.set(edit.view, goToDefinition);

  useLayoutEffect(() => {
    const view = edit.view;
    if (!view || !edit.at) return;
    moveCursor(view, edit.at);
    edit.at = null;
    view.focus();
  }, [edit, edit.at]);

  useLayoutEffect(() => {
    const view = edit.view;
    if (!view) return;
    view.dispatch({ effects: bindings.reconfigure(vimKeys ? vimBindings : []) });
    const cm = getCM(view);
    if (vimKeys && cm) cm.save = save;
  }, [edit, vimKeys, edit.generation]);

  useLayoutEffect(() => {
    if (findRequest && edit.view) openSearchPanel(edit.view);
  }, [edit, findRequest]);

  return <div className="agcode agcm" ref={host} onKeyDown={event => {
    if (vimKeys && event.key === "Escape" && !event.nativeEvent.isComposing) event.preventDefault();
    if (event.defaultPrevented) event.stopPropagation();
  }} />;
}

/// Search the source without entering edit mode, including files too large to edit.
export function SourceSearch({ text, findRequest, close }: { text: string; findRequest: number; close: (line: number | null) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const searchView = useRef<EditorView | null>(null);

  useLayoutEffect(() => {
    const parent = host.current;
    if (!parent) return;
    const view = new EditorView({ parent, doc: text, extensions: [
      theme, lineNumbers(), drawSelection(), fileSearch, keymap.of(searchKeymap), EditorState.readOnly.of(true), EditorView.editable.of(false),
      EditorView.updateListener.of(update => {
        if (searchPanelOpen(update.startState) && !searchPanelOpen(update.state)) {
          parent.closest<HTMLElement>(".agpane")?.focus({ preventScroll: true });
          const selection = update.state.selection.main;
          close(selection.empty ? null : update.state.doc.lineAt(selection.from).number);
        }
      }),
    ] });
    searchView.current = view;

    return () => { searchView.current = null; view.destroy(); };
  }, [text]);

  useLayoutEffect(() => {
    if (searchView.current) openSearchPanel(searchView.current);
  }, [text, findRequest]);

  return <div className="agcode agcm" ref={host} onKeyDown={event => { if (event.defaultPrevented) event.stopPropagation(); }} />;
}

function moveCursor(view: EditorView, at: TextPlace) {
  const { doc } = view.state;
  const line = doc.line(Math.min(Math.max(1, at.line + 1), doc.lines));
  const anchor = line.from + Math.min(Math.max(0, at.column), line.length);
  view.dispatch({ selection: { anchor }, effects: EditorView.scrollIntoView(anchor, { y: "center" }) });
}

const definitionHandlers = new WeakMap<EditorView, (request: DefinitionRequest) => void>();

function triggerDefinition(view: EditorView, pos: number, clientX: number, clientY: number): boolean {
  const handler = definitionHandlers.get(view);
  if (!handler) return false;
  const docLine = view.state.doc.lineAt(pos);
  const column = pos - docLine.from;
  if (!wordSpanAt(docLine.text, column)) return false;

  handler({ line: docLine.number - 1, column, text: view.state.sliceDoc(), clientX, clientY });
  return true;
}

const setLspLink = StateEffect.define<{ from: number; to: number } | null>();
const lspLinkMark = Decoration.mark({ class: "cm-lsp-link" });

interface LspLinkState {
  span: { from: number; to: number } | null;
  decorations: DecorationSet;
}

const EMPTY_LINK: LspLinkState = { span: null, decorations: Decoration.none };

const lspLinkField = StateField.define<LspLinkState>({
  create: () => EMPTY_LINK,
  update(value, tr) {
    let next = tr.docChanged ? EMPTY_LINK : value;
    for (const effect of tr.effects) {
      if (effect.is(setLspLink)) {
        next = effect.value
          ? { span: effect.value, decorations: Decoration.set([lspLinkMark.range(effect.value.from, effect.value.to)]) }
          : EMPTY_LINK;
      }
    }
    return next;
  },
  provide: field => EditorView.decorations.from(field, value => value.decorations),
});

function setHoverSpan(view: EditorView, next: { from: number; to: number } | null) {
  const cur = view.state.field(lspLinkField).span;
  if (cur?.from === next?.from && cur?.to === next?.to) return;
  view.dispatch({ effects: setLspLink.of(next) });
}

const lspHoverPlugin = ViewPlugin.fromClass(class {
  private readonly win: Window | null;
  private readonly clear: () => void;
  private readonly onKey: (event: KeyboardEvent) => void;

  constructor(view: EditorView) {
    this.win = view.dom.ownerDocument.defaultView;
    this.clear = () => setHoverSpan(view, null);
    this.onKey = event => { if (!primaryModifier(event)) this.clear(); };
    this.win?.addEventListener("keydown", this.onKey);
    this.win?.addEventListener("keyup", this.onKey);
    this.win?.addEventListener("blur", this.clear);
  }

  destroy() {
    this.win?.removeEventListener("keydown", this.onKey);
    this.win?.removeEventListener("keyup", this.onKey);
    this.win?.removeEventListener("blur", this.clear);
  }
}, {
  eventHandlers: {
    mousemove(event, view) {
      if (!primaryModifier(event)) { setHoverSpan(view, null); return false; }
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) { setHoverSpan(view, null); return false; }

      const docLine = view.state.doc.lineAt(pos);
      const span = wordSpanAt(docLine.text, pos - docLine.from);
      setHoverSpan(view, span ? { from: docLine.from + span.from, to: docLine.from + span.to } : null);
      return false;
    },
    mouseleave(_event, view) {
      setHoverSpan(view, null);
      return false;
    },
    mousedown(event, view) {
      if (event.button !== 0 || !primaryModifier(event)) return false;
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null || !triggerDefinition(view, pos, event.clientX, event.clientY)) return false;

      event.preventDefault();
      setHoverSpan(view, null);
      return true;
    },
  },
});

const lspInteraction = [lspLinkField, lspHoverPlugin];

const bindings = new Compartment();
Vim.defineAction("toggleFold", cm => { toggleFold(cm.cm6); });
Vim.mapCommand("za", "action", "toggleFold", {}, { context: "normal" });
Vim.defineAction("gotoDefinition", cm => {
  const view = cm.cm6;
  const head = view.state.selection.main.head;
  const coords = view.coordsAtPos(head);
  const rect = view.dom.getBoundingClientRect();
  triggerDefinition(view, head, coords?.left ?? rect.left + 20, coords?.bottom ?? rect.top + 20);
});
Vim.mapCommand("gd", "action", "gotoDefinition", {}, { context: "normal" });
const vimBindings = vim({ status: true });
const fileSearch = search({ top: true });
const language = new Compartment();

const theme = EditorView.theme({
  "&": { height: "100%", color: "var(--fg)", backgroundColor: "transparent", font: "11.5px/17px var(--mono)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "inherit", lineHeight: "inherit" },
  ".cm-content": { padding: "8px 0", caretColor: "var(--fg)" },
  ".cm-panels": { backgroundColor: "var(--surface)", color: "var(--fg)", fontFamily: "var(--mono)" },
  ".cm-panel input, .cm-panel button": { fontFamily: "inherit", color: "var(--fg)" },
  ".cm-textfield, .cm-button": { background: "var(--ctl-fill)", borderColor: "var(--border)" },
  ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent)" },
  ".cm-searchMatch-selected": { backgroundColor: "color-mix(in srgb, var(--accent) 52%, transparent)" },
  ".cm-cursor": { borderLeftColor: "var(--fg)" },
  ".cm-gutters": { backgroundColor: "var(--pane-bg, var(--ink))", color: "var(--dim)", border: "none" },
  ".cm-tooltip": { color: "var(--fg)", backgroundColor: "var(--glass-solid)", borderColor: "var(--border)" },
  ".cm-tooltip-autocomplete ul li[aria-selected]": { backgroundColor: "var(--ctl-fill-hi)", color: "var(--ink)" },
  ".cm-foldPlaceholder": { backgroundColor: "var(--ctl-fill)", color: "var(--muted)", border: "none" },
  ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--ctl-fill)" },
  // As specific as the base theme's selection, which is dark and hides the text on a light skin.
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionLayer .cm-selectionBackground": { backgroundColor: "color-mix(in srgb, var(--accent) 30%, transparent)" },
}, { dark: true });

function setup(text: string, changed: () => void, save: () => void, closeSearch: () => void) {
  return [
    // A file with Windows line ends keeps them.
    EditorState.lineSeparator.of(text.includes("\r\n") ? "\r\n" : "\n"),
    bindings.of([]), fileSearch,
    syntaxHighlighting(tokens), language.of([]), theme, lspInteraction,
    keymap.of([{ key: "Mod-s", run: () => { save(); return true; }, preventDefault: true }, indentWithTab]),
    basicSetup,
    EditorView.updateListener.of(update => {
      if (update.docChanged) changed();
      if (searchPanelOpen(update.startState) && !searchPanelOpen(update.state)) closeSearch();
    }),
  ];
}
