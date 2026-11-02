import { Dialog, Tabs } from "radix-ui";
import { useId, type ReactElement, type ReactNode } from "react";
import type {
  BlockedReport,
  Card,
  CaseFile,
  CaseIncident,
  ChildItem,
  ClosingSummary,
  Column,
  Ending,
  Link,
  PendingQuestion,
  RunStatus,
  StageChange,
  WorkerRun,
} from "../../api/types.gen";
import { IssueLink } from "../../components/IssueLink";
import { elapsed } from "../../time";
import { useCaseFile } from "./live";
import { PostmortemTab } from "../postmortems/PostmortemTab";

const COLUMNS: Record<Column, string> = {
  Backlog: "Backlog",
  Intake: "Intake",
  Planning: "Planning",
  Making: "Making",
  Checking: "Checking",
  Reviewing: "Reviewing",
  Merging: "Merging",
  NeedsRob: "Needs Rob",
  Done: "Done",
  Cancelled: "Cancelled",
};

const ENDINGS: Record<Ending, string> = {
  Merged: "Merged",
  Stopped: "Stopped",
  Replaced: "Replaced",
  Duplicate: "Duplicate",
  AlreadyDone: "Already done",
  NotActionable: "Not actionable",
};

const RUN_TONES: Record<RunStatus, string> = {
  Running: "text-info",
  Finished: "text-text",
  Failed: "text-danger",
  Cancelled: "text-muted",
};

const LINK_TEXT: Record<Link["kind"], string> = {
  Issue: "Issue",
  PullRequest: "Pull request",
  Branch: "Branch",
  MergeQueue: "Merge queue",
};

// Times are UTC, as the controller's own notes write them.
const TIME = new Intl.DateTimeFormat("en-GB", {
  timeZone: "UTC",
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

function Time({ ms }: { ms: number }) {
  return (
    <time dateTime={new Date(ms).toISOString()} className="font-mono text-2xs whitespace-nowrap text-faint">
      {TIME.format(ms)} UTC
    </time>
  );
}

function Chip(props: { tone: string; children: ReactNode }) {
  return (
    <span className={`inline-flex h-5 items-center rounded px-1.5 text-2xs font-medium whitespace-nowrap ${props.tone}`}>
      {props.children}
    </span>
  );
}

function Section(props: { title: string; count?: number; children: ReactNode }) {
  const heading = useId();
  return (
    <section aria-labelledby={heading} className="border-t border-border px-4 py-3">
      <div className="mb-2 flex items-center gap-2">
        <h3 id={heading} className="text-xs font-semibold tracking-wide text-muted uppercase">
          {props.title}
        </h3>
        {props.count !== undefined && (
          <span className="rounded bg-surface-2 px-1.5 font-mono text-2xs font-medium text-faint">{props.count}</span>
        )}
      </div>
      {props.children}
    </section>
  );
}

function Disclosure(props: { label: string; children: ReactNode }) {
  return (
    <details className="group mt-1">
      <summary className="w-fit cursor-pointer text-2xs text-muted select-none hover:text-text">{props.label}</summary>
      <div className="mt-1 rounded border border-border bg-surface-2/60 px-2 py-1.5 text-2xs text-muted">
        {props.children}
      </div>
    </details>
  );
}

function Header(props: {
  card: Card;
  links: Link[];
  title: (heading: ReactElement) => ReactElement;
  actions?: ReactNode;
}) {
  const { card, links } = props;
  return (
    // The right padding keeps a long title clear of the drawer's close button.
    <header className="pt-4 pr-12 pb-3 pl-4">
      <p className="flex items-center gap-1.5 text-2xs text-muted">
        <IssueLink repo={card.repo} issue={card.issue} />
        <Chip tone="bg-surface-2 text-muted">{COLUMNS[card.column]}</Chip>
        {card.ending !== null && <Chip tone="bg-surface-2 text-muted">{ENDINGS[card.ending]}</Chip>}
      </p>
      {props.title(<h2 className="mt-1 text-base font-semibold text-text">{card.title}</h2>)}
      <p className="mt-1 text-xs text-text">
        {card.phase}
        {card.waiting_on !== "none" && <span className="text-muted"> · waiting on {card.waiting_on}</span>}
      </p>
      {links.length > 0 && (
        <ul className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs">
          {links.map((link) => (
            <li key={link.kind}>
              <a
                href={link.url}
                aria-label={link.label}
                title={link.label}
                target="_blank"
                rel="noreferrer"
                className="text-accent underline-offset-2 hover:underline"
              >
                {LINK_TEXT[link.kind]}
              </a>
            </li>
          ))}
        </ul>
      )}
      {props.actions}
    </header>
  );
}

function Counters({ card }: { card: Card }) {
  const counters: Array<[string, ReactNode]> = [
    ["Attempt", card.attempt],
    // The kernel's round is 1 while a review's findings are being fixed.
    ["In a fix round", card.round > 0 ? "Yes" : "No"],
    ["Plan retries", card.retries.plan],
    ["Audit rounds", card.retries.audit],
    ["Run failures", card.retries.run_failures],
    ["Effect failures", card.retries.effect_failures],
    ["Credit", card.credit ?? "–"],
  ];
  return (
    <Section title="Counters">
      <dl className="grid grid-cols-4 gap-x-3 gap-y-2 sm:grid-cols-7">
        {counters.map(([term, value]) => (
          <div key={term} className="flex flex-col">
            <dt className="text-2xs text-faint">{term}</dt>
            <dd className="font-mono text-sm text-text">{value}</dd>
          </div>
        ))}
      </dl>
    </Section>
  );
}

function Question({ question }: { question: PendingQuestion }) {
  return (
    <Section title="Question for Rob">
      <p className="text-2xs text-muted">
        {question.kind} · question {question.id}
      </p>
      {!question.posted && (
        <p className="mt-1 text-xs text-danger">Posting this question to the issue failed; Rob hasn't seen it there.</p>
      )}
      <p className="mt-1 text-sm text-text">{question.obstacle}</p>
      <ol className="mt-2 flex flex-col gap-1.5">
        {question.options.map((option, index) => {
          const recommended = index === question.recommended_option;
          return (
            <li
              key={option.action}
              className={`rounded-md border px-2.5 py-1.5 ${recommended ? "border-accent bg-accent-soft" : "border-border"}`}
            >
              <p className="flex items-center gap-2 text-xs font-medium text-text">
                {option.action}
                {recommended && <Chip tone="bg-surface text-accent">Recommended</Chip>}
              </p>
              <p className="text-xs text-muted">{option.outcome}</p>
            </li>
          );
        })}
      </ol>
      <p className="mt-2 text-2xs text-faint">Why TL3 recommends it</p>
      <p className="text-xs text-text">{question.recommendation_reason}</p>
      <p className="mt-2 text-2xs text-faint">{question.how_to_answer}</p>
    </Section>
  );
}

function Timeline({ timeline }: { timeline: StageChange[] }) {
  return (
    <Section title="Timeline" count={timeline.length}>
      {timeline.length === 0 ? (
        <p className="text-xs text-faint">No stage changes recorded yet.</p>
      ) : (
        <ol className="flex flex-col">
          {timeline.map((change) => (
            <li
              key={`${change.revision}-${change.step}`}
              className="grid grid-cols-[auto_1fr] items-baseline gap-x-3 border-l border-border py-1 pl-3 sm:grid-cols-[9.5rem_5.5rem_1fr]"
            >
              <Time ms={change.at_ms} />
              <span className="text-2xs font-medium text-muted">{COLUMNS[change.column]}</span>
              {/* On a phone the step takes its own line under the time and stage. */}
              <span className="col-span-2 min-w-0 text-xs sm:col-span-1">
                <span className="text-text">{change.step}</span>
                <span className="text-muted"> · {change.cause}</span>
              </span>
            </li>
          ))}
        </ol>
      )}
    </Section>
  );
}

function Runs({ runs, now }: { runs: WorkerRun[]; now: number }) {
  return (
    <Section title="Worker runs" count={runs.length}>
      {runs.length === 0 ? (
        <p className="text-xs text-faint">No worker has run yet.</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs">
            <thead className="text-2xs text-faint">
              <tr>
                <th className="py-1 pr-2 font-normal">Role</th>
                <th className="py-1 pr-2 font-normal">Model</th>
                <th className="py-1 pr-2 font-normal">Duration</th>
                <th className="py-1 pr-2 font-normal">Tokens</th>
                <th className="py-1 pr-2 font-normal">Outcome</th>
                <th className="py-1 font-normal">
                  <span className="sr-only">Trace</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => (
                <tr key={run.key} className="border-t border-border align-baseline">
                  <td className="py-1 pr-2 text-text">{run.role}</td>
                  <td className="py-1 pr-2 text-muted">
                    {[run.model, run.effort].filter((part): part is string => part !== null).join(" · ") || "–"}
                  </td>
                  <td className="py-1 pr-2 font-mono text-muted">{elapsed((run.ended_ms ?? now) - run.started_ms)}</td>
                  <td className="py-1 pr-2 font-mono text-muted">
                    {run.tokens === null ? <span title="Not recorded yet; see the trace">–</span> : run.tokens}
                  </td>
                  <td className={`py-1 pr-2 ${RUN_TONES[run.status]}`}>{run.outcome}</td>
                  <td className="py-1 text-right">
                    {run.trace_url !== null && (
                      <a
                        href={run.trace_url}
                        aria-label={`MLflow traces for the ${run.role} run ${run.key}`}
                        target="_blank"
                        rel="noreferrer"
                        className="text-accent underline-offset-2 hover:underline"
                      >
                        Trace
                      </a>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Section>
  );
}

function Incidents({ incidents }: { incidents: CaseIncident[] }) {
  const ordered = [...incidents].sort((a, b) => Number(b.open) - Number(a.open));
  return (
    <Section title="Incidents" count={incidents.length}>
      <ul className="flex flex-col gap-2">
        {ordered.map((incident) => (
          <li key={incident.id} className="text-xs">
            <p className="flex flex-wrap items-center gap-1">
              <Chip tone={incident.open ? "bg-danger-soft text-danger" : "bg-surface-2 text-muted"}>
                {incident.open ? "Open" : "Cleared"}
              </Chip>
              <Chip tone="bg-surface-2 text-muted">{incident.cause === "Tl3" ? "TL3" : "Outside"}</Chip>
              {incident.needs_rob && <Chip tone="bg-info-soft text-info">Needs Rob</Chip>}
            </p>
            <p className="mt-1 text-text">{incident.summary}</p>
            <Disclosure label="Evidence">
              <dl className="grid grid-cols-[5rem_1fr] gap-x-2">
                <dt>Incident</dt>
                <dd className="font-mono">{incident.id}</dd>
                <dt>Source</dt>
                <dd>{incident.source}</dd>
                <dt>Class</dt>
                <dd className="font-mono">{incident.class}</dd>
                {incident.effect !== null && (
                  <>
                    <dt>Step</dt>
                    <dd className="font-mono">{incident.effect}</dd>
                  </>
                )}
                <dt>Holds work</dt>
                <dd>{incident.blocking ? "Yes" : "No"}</dd>
              </dl>
            </Disclosure>
          </li>
        ))}
      </ul>
    </Section>
  );
}

function Blocked({ blocked }: { blocked: BlockedReport[] }) {
  return (
    <Section title="Maker blocked" count={blocked.length}>
      <ul className="flex flex-col gap-2">
        {blocked.map((report, index) => (
          <li key={`${report.generation}-${index}`} className="text-xs">
            <p className="text-text">{report.evidence}</p>
            {report.generation !== null && <p className="text-2xs text-faint">Generation {report.generation}</p>}
            {report.report !== null && (
              <Disclosure label="Maker's report">
                <p className="whitespace-pre-wrap">{report.report}</p>
              </Disclosure>
            )}
          </li>
        ))}
      </ul>
    </Section>
  );
}

function Children({ items }: { items: ChildItem[] }) {
  return (
    <Section title="Child items" count={items.length}>
      <ul className="flex flex-col gap-1.5">
        {items.map((child, index) => (
          <li key={child.id ?? `piece-${index}`} className="text-xs">
            <p className="flex items-center gap-2">
              <span className="text-text">{child.title}</span>
              <Chip tone="bg-surface-2 text-muted">{child.stage}</Chip>
            </p>
            {child.files.length > 0 && <p className="font-mono text-2xs text-faint">{child.files.join(", ")}</p>}
          </li>
        ))}
      </ul>
    </Section>
  );
}

function Summary({ summary }: { summary: ClosingSummary }) {
  return (
    <Section title="Closing summary">
      <p className="text-xs">
        <Chip tone={summary.ending === "Merged" ? "bg-ok-soft text-ok" : "bg-surface-2 text-muted"}>
          {ENDINGS[summary.ending]}
        </Chip>
      </p>
      <p className="mt-1 text-sm text-text">{summary.outcome}</p>
      {summary.unmet_goals.length > 0 && (
        <ul className="mt-1 list-disc pl-4 text-xs text-muted">
          {summary.unmet_goals.map((goal) => (
            <li key={goal}>{goal}</li>
          ))}
        </ul>
      )}
      {summary.removed_changes !== null && (
        <p className="mt-1 text-xs whitespace-pre-wrap text-muted">{summary.removed_changes}</p>
      )}
      {summary.note !== null && <p className="mt-1 text-xs whitespace-pre-wrap text-muted">{summary.note}</p>}
    </Section>
  );
}

/** A request's case file. Evidence and model text sit behind disclosures. */
export function CaseFileView(props: {
  file: CaseFile;
  now: number;
  /** Wraps the title heading, so a dialog can be named by it. */
  title?: (heading: ReactElement) => ReactElement;
  /** What Rob can do with the request, shown under its links. */
  actions?: ReactNode;
  postmortem?: ReactNode;
}) {
  const { file, now } = props;
  const details = (
    <>
      <Counters card={file.card} />
      {file.question !== null && <Question question={file.question} />}
      <Timeline timeline={file.timeline} />
      <Runs runs={file.runs} now={now} />
      {file.incidents.length > 0 && <Incidents incidents={file.incidents} />}
      {file.blocked.length > 0 && <Blocked blocked={file.blocked} />}
      {file.children.length > 0 && <Children items={file.children} />}
      {file.summary !== null && <Summary summary={file.summary} />}
    </>
  );
  return (
    <article className="text-text">
      <Header
        card={file.card}
        links={file.links}
        title={props.title ?? ((heading) => heading)}
        actions={props.actions}
      />
      {props.postmortem ? (
        <Tabs.Root defaultValue={new URLSearchParams(window.location.search).get("tab") === "postmortem" ? "postmortem" : "case"}>
          <Tabs.List aria-label="Case file tabs" className="flex gap-1 border-t border-border px-4 pt-2">
            <Tabs.Trigger value="case" className="rounded-t px-3 py-2 text-xs text-muted data-[state=active]:bg-surface-2 data-[state=active]:text-text">
              Case file
            </Tabs.Trigger>
            <Tabs.Trigger value="postmortem" className="rounded-t px-3 py-2 text-xs text-muted data-[state=active]:bg-surface-2 data-[state=active]:text-text">
              Postmortem
            </Tabs.Trigger>
          </Tabs.List>
          <Tabs.Content value="case">{details}</Tabs.Content>
          <Tabs.Content value="postmortem">{props.postmortem}</Tabs.Content>
        </Tabs.Root>
      ) : details}
    </article>
  );
}

/** The case file in a drawer over the board, kept current from the board's stream. */
export function CaseDrawer(props: {
  id: number | null;
  card: Card | undefined;
  now: number;
  onClose: () => void;
  /** The request's actions, shown in full in the drawer's header. */
  actions?: (card: Card) => ReactNode;
}) {
  const { file, error } = useCaseFile(props.id, props.card);
  const shown = file?.card.id === props.id ? file : undefined;
  return (
    <Dialog.Root open={props.id !== null} onOpenChange={(open) => !open && props.onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/20" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed inset-y-0 right-0 flex w-full max-w-[720px] flex-col border-l border-border bg-surface shadow-lg outline-none"
        >
          <Dialog.Close
            aria-label="Close the case file"
            className="absolute top-3 right-3 inline-flex size-7 items-center justify-center rounded text-muted hover:bg-surface-2 hover:text-text"
          >
            <svg aria-hidden="true" viewBox="0 0 12 12" className="size-3 fill-none stroke-current">
              <path d="M3 3l6 6M9 3l-6 6" strokeWidth="1.4" />
            </svg>
          </Dialog.Close>
          <div className="min-h-0 flex-1 overflow-y-auto">
            {error !== null && (
              <p role="alert" className="mx-4 mt-12 rounded-md bg-danger-soft px-3 py-2 text-xs text-danger">
                {error}
              </p>
            )}
            {shown ? (
              <CaseFileView
                key={shown.card.id}
                file={shown}
                postmortem={<PostmortemTab id={shown.card.id} />}
                now={props.now}
                title={(heading) => <Dialog.Title asChild>{heading}</Dialog.Title>}
                actions={props.actions?.(shown.card)}
              />
            ) : (
              <Dialog.Title className="px-4 pt-12 text-sm font-normal text-muted">
                {error === null ? "Reading the case file…" : "Case file unavailable"}
              </Dialog.Title>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
