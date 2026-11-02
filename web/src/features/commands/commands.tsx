import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import type { Action, CommandRecord } from "../../api/types.gen";
import { commandId, fileCommand } from "./api";

/** A command as a card shows it: from this tab while it is sent, then from the stream. */
export interface Shown {
  id: string;
  action: Action;
  status: "sending" | "failed" | CommandRecord["status"];
  reason: string | null;
}

interface Sent {
  node: number;
  action: Action;
  /** Set when the console refused the request or couldn't be reached. */
  error: string | null;
}

interface Commands {
  /** File a command; returns its id. */
  run(node: number, action: Action): string;
  /** The command a request's card should show, if any. */
  latest(node: number): Shown | null;
  records: CommandRecord[];
}

const Context = createContext<Commands | null>(null);

export function useCommands(): Commands {
  const commands = useContext(Context);
  if (!commands) throw new Error("useCommands needs a CommandsProvider");
  return commands;
}

export function CommandsProvider(props: {
  records: CommandRecord[];
  file?: typeof fileCommand;
  children: ReactNode;
}) {
  const file = props.file ?? fileCommand;
  const [sent, setSent] = useState<Map<string, Sent>>(() => new Map());
  const run = useCallback(
    (node: number, action: Action) => {
      const id = commandId();
      setSent((current) => new Map(current).set(id, { node, action, error: null }));
      file(node, id, action).catch((error: unknown) => {
        const reason = error instanceof Error ? error.message : String(error);
        setSent((current) => new Map(current).set(id, { node, action, error: reason }));
      });
      return id;
    },
    [file],
  );
  const value = useMemo<Commands>(() => {
    const byId = new Map(props.records.map((record) => [record.id, record]));
    return {
      run,
      records: props.records,
      latest(node) {
        // This tab's newest command for the card; another tab's while it is pending.
        const mine = [...sent].filter(([, command]) => command.node === node).pop();
        if (mine) {
          const [id, command] = mine;
          const record = byId.get(id);
          if (record) return { id, action: command.action, status: record.status, reason: record.reason };
          return command.error === null
            ? { id, action: command.action, status: "sending", reason: null }
            : { id, action: command.action, status: "failed", reason: command.error };
        }
        const pending = props.records.find((record) => record.node === node && record.status === "pending");
        return pending?.action
          ? { id: pending.id, action: pending.action, status: pending.status, reason: null }
          : null;
      },
    };
  }, [props.records, run, sent]);
  return <Context.Provider value={value}>{props.children}</Context.Provider>;
}

export function actionLabel(action: Action): string {
  switch (action.kind) {
    case "pause":
      return "Pause";
    case "resume":
      return "Resume";
    case "stop":
      return "Stop";
    case "restart":
      return "Restart";
    case "clear_incident":
      return `Clear incident ${action.incident}`;
    case "answer":
      return `Answer to question ${action.question}`;
    case "defer":
      return `Let TL3 decide on question ${action.question}`;
    case "comment":
      return "Your answer";
  }
}

export function statusText(shown: Shown): string {
  const label = actionLabel(shown.action);
  switch (shown.status) {
    case "sending":
      return `${label}: sending`;
    case "pending":
      return `${label}: pending`;
    case "applied":
      return `${label}: applied`;
    case "refused":
      return `${label} refused: ${shown.reason ?? "no reason given"}`;
    case "failed":
      return `${label} not sent: ${shown.reason ?? "no reason given"}`;
  }
}
