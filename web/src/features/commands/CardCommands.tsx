import { PostmortemButton } from "../postmortems/PostmortemButton";
import { DropdownMenu } from "radix-ui";
import { useEffect, useRef, useState } from "react";
import type { Action, Card, CardQuestion, Ending, QuestionOption } from "../../api/types.gen";
import { statusText, useCommands } from "./commands";

/** Q49: restart is offered until the work merges or is already being replaced. */
const RESTARTABLE: Ending[] = ["Stopped", "Duplicate", "AlreadyDone", "NotActionable"];

const BUTTON =
  "inline-flex h-6 items-center rounded border border-border bg-surface px-2 text-2xs font-medium text-text hover:border-border-strong disabled:cursor-not-allowed disabled:opacity-50";
const PRIMARY =
  "inline-flex h-6 items-center rounded bg-accent px-2 text-2xs font-medium text-white hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50";

const ITEM =
  "cursor-default rounded px-2 py-1.5 text-text outline-none data-[highlighted]:bg-accent-soft";
/** Where focus returns when a compact card's confirmation closes. */
const MENU = "menu";

type Steer = Extract<Action, { kind: "pause" | "resume" | "stop" | "restart" }>;

function steering(card: Card): Steer[] {
  if (card.ending !== null) {
    return RESTARTABLE.includes(card.ending) ? [{ kind: "restart" }] : [];
  }
  return [card.badges.includes("Paused") ? { kind: "resume" } : { kind: "pause" }, { kind: "stop" }, { kind: "restart" }];
}

const VERB: Record<Steer["kind"], string> = { pause: "Pause", resume: "Resume", stop: "Stop", restart: "Restart" };

/**
 * Pause, resume, stop and restart, each confirmed in one step. Focus moves
 * into the confirmation and back to what opened it, so the keyboard never
 * loses its place when the buttons swap. Compact cards keep these behind one
 * menu, with the incident clears.
 */
function Steering({ card, compact }: { card: Card; compact: boolean }) {
  const { run } = useCommands();
  const [armed, setArmed] = useState<Steer | null>(null);
  const [returnTo, setReturnTo] = useState<string | null>(null);
  const confirm = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLButtonElement>(null);
  const triggers = useRef(new Map<string, HTMLButtonElement>());
  // A menu choice opens the confirmation; the menu mustn't take focus back.
  const arming = useRef(false);
  useEffect(() => {
    if (armed) confirm.current?.focus();
  }, [armed]);
  useEffect(() => {
    if (armed || returnTo === null) return;
    const target = returnTo === MENU ? menu.current : (triggers.current.get(returnTo) ?? triggers.current.values().next().value);
    target?.focus();
    setReturnTo(null);
  }, [armed, returnTo]);
  const actions = steering(card);
  const close = (apply: boolean) => {
    if (!armed) return;
    if (apply) run(card.id, armed);
    setReturnTo(compact ? MENU : armed.kind);
    setArmed(null);
  };
  const confirmation = armed && (
    <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Confirm">
      <span className="text-2xs text-text">{VERB[armed.kind]} this request?</span>
      <button ref={confirm} type="button" className={PRIMARY} onClick={() => close(true)}>
        Confirm {VERB[armed.kind].toLowerCase()}
      </button>
      <button type="button" className={BUTTON} onClick={() => close(false)}>
        Cancel
      </button>
    </div>
  );
  if (compact) {
    if (actions.length === 0 && card.incidents.length === 0) return null;
    return (
      <>
        <DropdownMenu.Root modal={false}>
          <DropdownMenu.Trigger ref={menu} aria-label="Actions" className={`${BUTTON} px-1.5`}>
            <span aria-hidden="true">⋯</span>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              align="end"
              sideOffset={4}
              onCloseAutoFocus={(event) => {
                // Radix returns focus after the menu closes; send it to the confirmation instead.
                if (arming.current) {
                  arming.current = false;
                  event.preventDefault();
                  confirm.current?.focus();
                }
              }}
              className="z-10 min-w-36 rounded-md border border-border bg-surface p-1 text-xs shadow-lg"
            >
              {actions.map((action) => (
                <DropdownMenu.Item
                  key={action.kind}
                  className={ITEM}
                  onSelect={() => {
                    arming.current = true;
                    setArmed(action);
                  }}
                >
                  {VERB[action.kind]}
                </DropdownMenu.Item>
              ))}
              {card.incidents.map((incident) => (
                <DropdownMenu.Item
                  key={incident.id}
                  className={ITEM}
                  onSelect={() => run(card.id, { kind: "clear_incident", incident: incident.id })}
                >
                  Clear incident {incident.id}
                </DropdownMenu.Item>
              ))}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
        {confirmation}
      </>
    );
  }
  if (actions.length === 0) return null;
  if (confirmation) return confirmation;
  return (
    <div className="flex flex-wrap gap-1.5" role="group" aria-label="Actions">
      {actions.map((action) => (
        <button
          key={action.kind}
          ref={(button) => {
            if (button) triggers.current.set(action.kind, button);
            else triggers.current.delete(action.kind);
          }}
          type="button"
          className={BUTTON}
          onClick={() => setArmed(action)}
        >
          {VERB[action.kind]}
        </button>
      ))}
    </div>
  );
}

/** What an option still needs from Rob, and the command it files once given. */
function DetailForm(props: { option: QuestionOption; onSend: (value: string) => void; onCancel: () => void }) {
  const [value, setValue] = useState("");
  const amount = props.option.detail === "amount";
  const label =
    props.option.detail === "amount" ? "Credit (1–100)" : props.option.detail === "guidance" ? "Your guidance" : "Your new request";
  const ready = amount ? /^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= 100 : value.trim() !== "";
  const input = useRef<HTMLInputElement>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => (input.current ?? area.current)?.focus(), []);
  return (
    <form
      className="mt-1.5 flex flex-col gap-1.5"
      onSubmit={(event) => {
        event.preventDefault();
        if (ready) props.onSend(value);
      }}
    >
      <label className="flex flex-col gap-1 text-2xs text-muted">
        {label}
        {amount ? (
          <input
            ref={input}
            type="number"
            min={1}
            max={100}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            className="h-7 w-24 rounded border border-border bg-surface px-2 text-xs text-text"
          />
        ) : (
          <textarea
            ref={area}
            rows={3}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            className="rounded border border-border bg-surface px-2 py-1 text-xs text-text"
          />
        )}
      </label>
      <div className="flex gap-1.5">
        <button type="submit" className={PRIMARY} disabled={!ready}>
          Send {props.option.label.toLowerCase()}
        </button>
        <button type="button" className={BUTTON} onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

function answer(question: CardQuestion, option: QuestionOption, detail = ""): Action {
  switch (option.answer) {
    case "try_again":
      return { kind: "answer", question: question.id, choice: { answer: "try_again", guidance: detail } };
    case "add_credit":
      return { kind: "answer", question: question.id, choice: { answer: "add_credit", amount: Number(detail) } };
    case "change_request":
      return { kind: "answer", question: question.id, choice: { answer: "change_request", text: detail } };
    default:
      return { kind: "answer", question: question.id, choice: { answer: option.answer } };
  }
}

/** The open question in plain words, its options, TL3's recommendation and Rob's own words. */
export function QuestionPanel({ card, question }: { card: Card; question: CardQuestion }) {
  const { run } = useCommands();
  const [detail, setDetail] = useState<QuestionOption | null>(null);
  const [returnTo, setReturnTo] = useState<string | null>(null);
  const optionButtons = useRef(new Map<string, HTMLButtonElement>());
  const [words, setWords] = useState("");
  const answerBox = useRef<HTMLTextAreaElement>(null);
  const recommended = question.options[question.recommended];
  // Closing an option's form returns focus to the option that opened it.
  useEffect(() => {
    if (detail || returnTo === null) return;
    optionButtons.current.get(returnTo)?.focus();
    setReturnTo(null);
  }, [detail, returnTo]);
  const closeDetail = () => {
    setReturnTo(detail?.answer ?? null);
    setDetail(null);
  };
  return (
    <section aria-label={`Question ${question.id}`} className="mt-2 rounded border border-info/30 bg-info-soft/40 p-2">
      <p className="text-xs text-text">
        <span className="font-medium">Question {question.id}.</span> {question.prompt}
      </p>
      <ul className="mt-1.5 flex flex-col gap-1" aria-label="Options">
        {question.options.map((option, index) => (
          <li key={option.answer}>
            <button
              ref={(button) => {
                if (button) optionButtons.current.set(option.answer, button);
                else optionButtons.current.delete(option.answer);
              }}
              type="button"
              className={`${BUTTON} h-auto min-h-6 w-full flex-col items-stretch gap-0.5 py-1 text-left`}
              onClick={() => (option.detail ? setDetail(option) : run(card.id, answer(question, option)))}
            >
              <span className="flex items-center justify-between gap-2">
                <span>{option.label}</span>
                {index === question.recommended && (
                  <span className="rounded bg-accent-soft px-1 text-2xs font-medium text-accent">Recommended</span>
                )}
              </span>
              <span className="font-normal text-muted">{option.outcome}</span>
            </button>
          </li>
        ))}
      </ul>
      {detail && (
        <DetailForm
          option={detail}
          onCancel={closeDetail}
          onSend={(value) => {
            run(card.id, answer(question, detail, value));
            closeDetail();
          }}
        />
      )}
      {recommended && (
        <p className="mt-1.5 text-2xs text-muted">
          TL3 recommends “{recommended.label}”: {question.reason}
        </p>
      )}
      <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          className={PRIMARY}
          disabled={question.defer_blocked !== null}
          title={question.defer_blocked ?? `Apply “${recommended?.label}” and note that you deferred`}
          onClick={() => run(card.id, { kind: "defer", question: question.id })}
        >
          Let TL3 decide
        </button>
        {question.defer_blocked && <span className="text-2xs text-muted">{question.defer_blocked}</span>}
      </div>
      <details className="mt-1.5">
        <summary className="cursor-pointer text-2xs text-muted">Write my own answer</summary>
        <form
          className="mt-1 flex flex-col gap-1.5"
          onSubmit={(event) => {
            event.preventDefault();
            if (words.trim() === "") return;
            run(card.id, { kind: "comment", text: words });
            setWords("");
            // Clearing the box disables its button, which would drop focus to the page.
            answerBox.current?.focus();
          }}
        >
          <textarea
            ref={answerBox}
            aria-label="Your answer"
            rows={3}
            value={words}
            onChange={(event) => setWords(event.target.value)}
            className="rounded border border-border bg-surface px-2 py-1 text-xs text-text"
          />
          <button type="submit" className={`${BUTTON} self-start`} disabled={words.trim() === ""}>
            Send answer
          </button>
        </form>
      </details>
    </section>
  );
}

/**
 * Everything Rob can do from a card, and how his last command settled.
 * Compact cards keep the actions behind one menu; Needs Rob cards show them.
 */
export function CardCommands({ card, compact = false }: { card: Card; compact?: boolean }) {
  const { run, latest } = useCommands();
  const shown = latest(card.id);
  const tone =
    shown?.status === "refused" || shown?.status === "failed"
      ? "text-danger"
      : shown?.status === "applied"
        ? "text-ok"
        : "text-muted";
  // The live region stays mounted, so the first status is announced too.
  const status = (
    <p aria-live="polite" className={shown ? `min-w-0 flex-1 text-2xs ${tone}` : "sr-only"}>
      {shown ? statusText(shown) : ""}
    </p>
  );
  if (compact) {
    return (
      <div className="mt-2 flex flex-col gap-1.5">
        {card.question && <QuestionPanel card={card} question={card.question} />}
        <div className="flex items-center justify-end gap-2">
          {status}
          <Steering card={card} compact />
        </div>
      </div>
    );
  }
  return (
    <div className="mt-2 flex flex-col gap-1.5 border-t border-border pt-2">
      {card.question && <QuestionPanel card={card} question={card.question} />}
      {card.incidents.map((incident) => (
        <div key={incident.id} className="flex items-center gap-1.5 text-2xs text-muted">
          <span className="min-w-0 flex-1 truncate" title={incident.summary}>
            <span className="font-mono">{incident.id}</span> {incident.summary}
          </span>
          <button
            type="button"
            className={BUTTON}
            onClick={() => run(card.id, { kind: "clear_incident", incident: incident.id })}
          >
            Clear
          </button>
        </div>
      ))}
      <Steering card={card} compact={false} />
      <PostmortemButton card={card} />
      {status}
    </div>
  );
}
