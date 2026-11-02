import { useState } from "react";
import type { Action, Card, CardQuestion, Ending, QuestionOption } from "../../api/types.gen";
import { statusText, useCommands } from "./commands";

/** Q49: restart is offered until the work merges or is already being replaced. */
const RESTARTABLE: Ending[] = ["Stopped", "Duplicate", "AlreadyDone", "NotActionable"];

const BUTTON =
  "inline-flex h-6 items-center rounded border border-border bg-surface px-2 text-2xs font-medium text-text hover:border-border-strong disabled:cursor-not-allowed disabled:opacity-50";
const PRIMARY =
  "inline-flex h-6 items-center rounded bg-accent px-2 text-2xs font-medium text-white hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50";

type Steer = Extract<Action, { kind: "pause" | "resume" | "stop" | "restart" }>;

function steering(card: Card): Steer[] {
  if (card.ending !== null) {
    return RESTARTABLE.includes(card.ending) ? [{ kind: "restart" }] : [];
  }
  return [card.badges.includes("Paused") ? { kind: "resume" } : { kind: "pause" }, { kind: "stop" }, { kind: "restart" }];
}

const VERB: Record<Steer["kind"], string> = { pause: "Pause", resume: "Resume", stop: "Stop", restart: "Restart" };

/** Pause, resume, stop and restart, each confirmed in one step. */
function Steering({ card }: { card: Card }) {
  const { run } = useCommands();
  const [armed, setArmed] = useState<Steer | null>(null);
  const actions = steering(card);
  if (actions.length === 0) return null;
  if (armed) {
    return (
      <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Confirm">
        <span className="text-2xs text-text">{VERB[armed.kind]} this request?</span>
        <button
          type="button"
          className={PRIMARY}
          onClick={() => {
            run(card.id, armed);
            setArmed(null);
          }}
        >
          Confirm {VERB[armed.kind].toLowerCase()}
        </button>
        <button type="button" className={BUTTON} onClick={() => setArmed(null)}>
          Cancel
        </button>
      </div>
    );
  }
  return (
    <div className="flex flex-wrap gap-1.5" role="group" aria-label="Actions">
      {actions.map((action) => (
        <button key={action.kind} type="button" className={BUTTON} onClick={() => setArmed(action)}>
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
            type="number"
            min={1}
            max={100}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            className="h-7 w-24 rounded border border-border bg-surface px-2 text-xs text-text"
          />
        ) : (
          <textarea
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
  const [words, setWords] = useState("");
  const recommended = question.options[question.recommended];
  return (
    <section aria-label={`Question ${question.id}`} className="mt-2 rounded border border-info/30 bg-info-soft/40 p-2">
      <p className="text-xs text-text">
        <span className="font-medium">Question {question.id}.</span> {question.prompt}
      </p>
      <ul className="mt-1.5 flex flex-col gap-1" aria-label="Options">
        {question.options.map((option, index) => (
          <li key={option.answer}>
            <button
              type="button"
              title={option.outcome}
              className={`${BUTTON} h-auto min-h-6 w-full justify-between gap-2 py-1 text-left`}
              onClick={() => (option.detail ? setDetail(option) : run(card.id, answer(question, option)))}
            >
              <span>{option.label}</span>
              {index === question.recommended && (
                <span className="rounded bg-accent-soft px-1 text-2xs font-medium text-accent">Recommended</span>
              )}
            </button>
          </li>
        ))}
      </ul>
      {detail && (
        <DetailForm
          option={detail}
          onCancel={() => setDetail(null)}
          onSend={(value) => {
            run(card.id, answer(question, detail, value));
            setDetail(null);
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
          }}
        >
          <textarea
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

/** Everything Rob can do from a card, and how his last command settled. */
export function CardCommands({ card }: { card: Card; compact?: boolean }) {
  const { run, latest } = useCommands();
  const shown = latest(card.id);
  const tone =
    shown?.status === "refused" || shown?.status === "failed"
      ? "text-danger"
      : shown?.status === "applied"
        ? "text-ok"
        : "text-muted";
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
      <Steering card={card} />
      {shown && (
        <p aria-live="polite" data-command={shown.status} className={`text-2xs ${tone}`}>
          {statusText(shown)}
        </p>
      )}
    </div>
  );
}
