// CodeMirror for a text tab being edited; loaded in its own chunk on the first edit.
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, HighlightStyle, indentOnInput, LanguageDescription, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightSelectionMatches } from "@codemirror/search";
import { Compartment, EditorState } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { useLayoutEffect, useRef } from "react";
import type { FileEdit } from "./media.ts";

export default function CodeEditor({ edit, rel, changed, save }: { edit: FileEdit; rel: string; changed: () => void; save: () => void }) {
  const host = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const parent = host.current;
    if (!parent) return;

    const state = edit.state ?? EditorState.create({ doc: edit.base, extensions: setup(edit.base, changed, save) });
    const view = new EditorView({ parent, state });
    edit.view = view;
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

  return <div className="agcode agcm" ref={host} />;
}

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
  ".cm-cursor": { borderLeftColor: "var(--fg)" },
  ".cm-gutters": { backgroundColor: "transparent", color: "var(--dim)", border: "none" },
  ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--ctl-fill)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "color-mix(in srgb, var(--accent) 30%, transparent)" },
}, { dark: true });

function setup(text: string, changed: () => void, save: () => void) {
  return [
    // A file with Windows line ends keeps them.
    EditorState.lineSeparator.of(text.includes("\r\n") ? "\r\n" : "\n"),
    lineNumbers(), highlightActiveLineGutter(), highlightSpecialChars(), history(), drawSelection(),
    indentOnInput(), bracketMatching(), highlightActiveLine(), highlightSelectionMatches(),
    syntaxHighlighting(tokens), language.of([]), theme,
    keymap.of([{ key: "Mod-s", run: () => { save(); return true; }, preventDefault: true }, indentWithTab, ...defaultKeymap, ...historyKeymap]),
    EditorView.updateListener.of(update => { if (update.docChanged) changed(); }),
  ];
}
