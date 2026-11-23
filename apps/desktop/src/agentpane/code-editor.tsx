// CodeMirror for a text tab being edited; loaded in its own chunk on the first edit.
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, HighlightStyle, indentOnInput, LanguageDescription, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightSelectionMatches, openSearchPanel, search, searchKeymap, searchPanelOpen } from "@codemirror/search";
import { Compartment, EditorState } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { getCM, vim } from "@replit/codemirror-vim";
import { useLayoutEffect, useRef } from "react";
import type { FileEdit } from "./media.ts";

export default function CodeEditor({ edit, rel, changed, save, vimKeys, findRequest, closeSearch }: {
  edit: FileEdit; rel: string; changed: () => void; save: () => void; vimKeys: boolean; findRequest: number; closeSearch: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const parent = host.current;
    if (!parent) return;

    const state = edit.state ?? EditorState.create({ doc: edit.base, extensions: setup(edit.base, changed, save, closeSearch) });
    const view = new EditorView({ parent, state });
    edit.view = view;

    // A new editor opened at a place puts the cursor there.
    if (!edit.state && edit.at) {
      const { doc } = view.state, line = doc.line(Math.min(edit.at.line + 1, doc.lines));
      const anchor = line.from + Math.min(edit.at.column, line.length);
      view.dispatch({ selection: { anchor }, effects: EditorView.scrollIntoView(anchor, { y: "center" }) });
      edit.at = null;
    }

    view.focus();

    // The language's parser loads on its own, after the text shows.
    let live = true;
    void LanguageDescription.matchFilename(languages, rel)?.load().then(support => {
      if (live) view.dispatch({ effects: language.reconfigure(support) });
    });

    return () => {
      live = false;
      // A reload let go of this editor already, and its text with it.
      if (edit.view === view) { edit.state = view.state; edit.view = null; }
      view.destroy();
    };
    // The callbacks act on the tab whose edit this is, so the first ones do.
  }, [edit, rel]);

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
      theme, lineNumbers(), drawSelection(), fileSearch, EditorState.readOnly.of(true), EditorView.editable.of(false),
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

const bindings = new Compartment();
const vimBindings = vim({ status: true });
const fileSearch = [search({ top: true }), keymap.of(searchKeymap)];
const language = new Compartment();

/// The token classes the read-only view colours, so both look the same.
const tokens = HighlightStyle.define([
  { tag: t.keyword, class: "k" },
  { tag: [t.string, t.special(t.string), t.regexp], class: "s" },
  { tag: t.comment, class: "c" },
  { tag: [t.number, t.bool, t.null], class: "n" },
]);

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
  ".cm-gutters": { backgroundColor: "transparent", color: "var(--dim)", border: "none" },
  ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--ctl-fill)" },
  // As specific as the base theme's selection, which is dark and hides the text on a light skin.
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionLayer .cm-selectionBackground": { backgroundColor: "color-mix(in srgb, var(--accent) 30%, transparent)" },
}, { dark: true });

function setup(text: string, changed: () => void, save: () => void, closeSearch: () => void) {
  return [
    // A file with Windows line ends keeps them.
    EditorState.lineSeparator.of(text.includes("\r\n") ? "\r\n" : "\n"),
    bindings.of([]), fileSearch,
    lineNumbers(), highlightActiveLineGutter(), highlightSpecialChars(), history(), drawSelection(),
    indentOnInput(), bracketMatching(), highlightActiveLine(), highlightSelectionMatches(),
    syntaxHighlighting(tokens), language.of([]), theme,
    keymap.of([{ key: "Mod-s", run: () => { save(); return true; }, preventDefault: true }, indentWithTab, ...defaultKeymap, ...historyKeymap]),
    EditorView.updateListener.of(update => {
      if (update.docChanged) changed();
      if (searchPanelOpen(update.startState) && !searchPanelOpen(update.state)) closeSearch();
    }),
  ];
}
