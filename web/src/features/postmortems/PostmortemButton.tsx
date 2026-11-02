import { useEffect, useId, useRef, useState } from "react";
import type { Card } from "../../api/types.gen";
import { useCommands } from "../commands/commands";
import { usePostmortems } from "./api";
import { BUTTON } from "./PostmortemTab";

export function PostmortemButton({ card }: { card: Card }) {
  const eligible = card.ending !== null || card.column === "NeedsRob";
  const query = usePostmortems(eligible ? card.id : null);
  const { run, latest } = useCommands();
  const shown = latest(card.id);
  const pending = shown?.action.kind === "postmortem" && (shown.status === "pending" || shown.status === "sending");
  const active =
    query.data?.pages[0]?.active ||
    query.data?.pages.some((page) =>
      page.postmortems.some((item) => item.status === "owed" || item.status === "running"),
    );
  const reason = active
    ? "A postmortem for this request is already owed or running."
    : pending
      ? "Waiting for TL3 to apply your request."
      : query.error
        ? "Postmortem status is unavailable."
        : !query.data
          ? "Reading postmortem status…"
          : null;
  const [armed, setArmed] = useState(false);
  const confirm = useRef<HTMLButtonElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const description = useId();
  useEffect(() => {
    if (armed) confirm.current?.focus();
  }, [armed]);
  if (!eligible) return null;
  const close = () => {
    setArmed(false);
    queueMicrotask(() => trigger.current?.focus());
  };
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {armed && reason === null ? (
        <div
          role="group"
          aria-label="Confirm postmortem"
          className="flex flex-wrap items-center gap-1.5"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.stopPropagation();
              close();
            }
          }}
        >
          <span className="text-2xs text-muted">Write a postmortem for this request?</span>
          <button
            ref={confirm}
            className={BUTTON}
            onClick={() => {
              run(card.id, { kind: "postmortem" });
              close();
            }}
          >
            Confirm postmortem
          </button>
          <button className={BUTTON} onClick={close}>
            Cancel
          </button>
        </div>
      ) : (
        <button
          ref={trigger}
          className={BUTTON}
          disabled={reason !== null}
          aria-describedby={reason ? description : undefined}
          onClick={() => setArmed(true)}
        >
          Write postmortem
        </button>
      )}
      {reason && (
        <span id={description} className="text-2xs text-muted">
          {reason}
        </span>
      )}
      {query.error && (
        <button className={BUTTON} onClick={() => void query.refetch()}>
          Retry status
        </button>
      )}
    </div>
  );
}
