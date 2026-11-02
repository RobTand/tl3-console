import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { unifiedMergeView } from "@codemirror/merge";
import { EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";

// Tailwind inlines its font tokens, so these repeat styles.css's stacks.
const MONO = 'ui-monospace, "SF Mono", "JetBrains Mono", Menlo, monospace';
const SANS = '"Inter", ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif';

/** The console's colour tokens, so both themes follow the page's. */
const chrome = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--surface)", color: "var(--text)", fontSize: "13px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: MONO, lineHeight: "20px", overflow: "auto" },
  ".cm-content": { padding: "8px 0", caretColor: "var(--text)" },
  ".cm-line": { padding: "0 12px" },
  ".cm-gutters": { backgroundColor: "var(--surface-2)", color: "var(--text-faint)", border: "none" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--accent-soft) 45%, transparent)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--accent-soft)", color: "var(--text-muted)" },
  "&.cm-focused .cm-cursor": { borderLeftColor: "var(--text)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--accent-soft)",
  },
  ".cm-changedLine, .cm-insertedLine": { backgroundColor: "var(--ok-soft)" },
  ".cm-changedText": { background: "color-mix(in srgb, var(--ok) 28%, transparent)" },
  ".cm-deletedChunk": { backgroundColor: "var(--danger-soft)" },
  ".cm-deletedChunk .cm-deletedText, .cm-deletedChunk del": {
    background: "color-mix(in srgb, var(--danger) 25%, transparent)",
    textDecoration: "none",
  },
  ".cm-changeGutter": { width: "3px", paddingLeft: "1px" },
  ".cm-changedLineGutter, .cm-insertedLineGutter": { backgroundColor: "var(--ok)" },
  ".cm-deletedLineGutter": { backgroundColor: "var(--danger)" },
  ".cm-collapsedLines": {
    color: "var(--text-muted)",
    background: "var(--surface-2)",
    fontFamily: SANS,
    fontSize: "12px",
    padding: "2px 12px",
  },
});

const markdownStyle = HighlightStyle.define([
  { tag: tags.heading, fontWeight: "600", color: "var(--text)" },
  { tag: tags.strong, fontWeight: "600" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: [tags.link, tags.url], color: "var(--accent)" },
  { tag: [tags.monospace, tags.processingInstruction], color: "var(--info)" },
  { tag: [tags.list, tags.quote, tags.contentSeparator, tags.meta], color: "var(--text-muted)" },
]);

const common = (label: string): Extension[] => [
  lineNumbers(),
  EditorView.lineWrapping,
  markdown(),
  syntaxHighlighting(markdownStyle),
  chrome,
  EditorView.contentAttributes.of({ "aria-label": label }),
];

export function editorState(text: string, label: string, onChange: (text: string) => void): EditorState {
  return EditorState.create({
    doc: text,
    extensions: [
      ...common(label),
      highlightActiveLineGutter(),
      highlightActiveLine(),
      history(),
      drawSelection(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) onChange(update.state.doc.toString());
      }),
    ],
  });
}

/** `after` with `before`'s lines shown as deletions, read-only. */
export function diffState(before: string, after: string, label: string): EditorState {
  return EditorState.create({
    doc: after,
    extensions: [
      ...common(label),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      unifiedMergeView({ original: before, mergeControls: false, collapseUnchanged: { margin: 3, minSize: 8 } }),
    ],
  });
}
