import { EditorView } from "@codemirror/view";
import { useEffect, useRef } from "react";
import { diffState, editorState } from "./codemirror";

const FRAME = "h-full min-h-0 overflow-hidden rounded-md border border-border bg-surface";

/** A Markdown editor that starts from `initial`; remount it to start again. */
export function PromptEditor(props: { initial: string; label: string; onChange: (text: string) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const onChange = useRef(props.onChange);
  useEffect(() => {
    onChange.current = props.onChange;
  });
  useEffect(() => {
    const view = new EditorView({
      parent: host.current!,
      state: editorState(props.initial, props.label, (text) => onChange.current(text)),
    });
    return () => view.destroy();
  }, [props.initial, props.label]);
  return <div ref={host} className={FRAME} data-testid="prompt-editor" />;
}

/** `after` against `before`: added lines highlighted, removed lines shown inline. */
export function PromptDiff(props: { before: string; after: string; label: string }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const view = new EditorView({ parent: host.current!, state: diffState(props.before, props.after, props.label) });
    return () => view.destroy();
  }, [props.before, props.after, props.label]);
  return <div ref={host} className={FRAME} data-testid="prompt-diff" />;
}
