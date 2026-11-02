import { Dialog } from "radix-ui";
import { useState } from "react";
import type { Card } from "../../api/types.gen";
import { useCommands } from "./commands";

/**
 * "Let TL3 decide on all" for the Needs Rob column: lists what each question
 * will get before Rob confirms, then defers each one. tl3d skips, with a
 * reason, any question answered in the meantime. With a filter on, it
 * covers only the questions shown, and says so.
 */
export function DeferAll({ cards, filtered = false }: { cards: Card[]; filtered?: boolean }) {
  const { run } = useCommands();
  const [open, setOpen] = useState(false);
  const asked = cards.filter((card) => card.question !== null);
  const deferrable = asked.filter((card) => card.question!.defer_blocked === null);
  const label = filtered ? `Let TL3 decide on all shown (${asked.length})` : "Let TL3 decide on all";
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger
        disabled={asked.length === 0}
        className="rounded border border-border bg-surface px-1.5 py-0.5 text-2xs font-medium tracking-normal text-text normal-case hover:border-border-strong disabled:cursor-not-allowed disabled:opacity-50"
      >
        {label}
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed top-1/2 left-1/2 max-h-[80vh] w-[min(36rem,92vw)] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-lg border border-border bg-surface p-4 shadow-lg">
          <Dialog.Title className="text-sm font-semibold text-text">{label}</Dialog.Title>
          <Dialog.Description className="mt-1 text-xs text-muted">
            TL3 applies its recommended option to each question below and notes on each issue that you deferred.
          </Dialog.Description>
          <ul className="mt-3 flex flex-col gap-2" aria-label="Choices">
            {asked.map((card) => {
              const question = card.question!;
              const option = question.options[question.recommended];
              return (
                <li key={card.id} className="rounded border border-border px-2.5 py-2 text-xs">
                  <p className="text-text">
                    <span className="font-mono text-faint">
                      {card.repo.split("/")[1] ?? card.repo}#{card.issue}
                    </span>{" "}
                    {card.title}
                  </p>
                  {question.defer_blocked === null ? (
                    <p className="mt-0.5 text-muted">
                      Question {question.id}: <span className="font-medium text-text">{option?.label}</span>
                    </p>
                  ) : (
                    <p className="mt-0.5 text-warn">Skipped: {question.defer_blocked}</p>
                  )}
                </li>
              );
            })}
          </ul>
          <div className="mt-4 flex justify-end gap-2">
            <Dialog.Close className="h-8 rounded-md border border-border bg-surface px-3 text-xs text-text hover:border-border-strong">
              Cancel
            </Dialog.Close>
            <button
              type="button"
              disabled={deferrable.length === 0}
              className="h-8 rounded-md bg-accent px-3 text-xs font-medium text-white hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              onClick={() => {
                for (const card of deferrable) run(card.id, { kind: "defer", question: card.question!.id });
                setOpen(false);
              }}
            >
              Apply {deferrable.length} {deferrable.length === 1 ? "choice" : "choices"}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
