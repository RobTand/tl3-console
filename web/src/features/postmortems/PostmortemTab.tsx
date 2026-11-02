import type { Postmortem, PostmortemReason, PostmortemStatus } from "../../api/types.gen";
import { usePostmortems } from "./api";

export const REASONS: Record<PostmortemReason, string> = {
  needed_fix: "Needed a fix",
  retry: "Retried work",
  escalation: "Needed Rob",
  closed_without_merge: "Closed without merging",
  sampled: "Sampled clean work",
  rob_asked: "Rob asked",
};
const STATUS: Record<PostmortemStatus, string> = { owed: "Owed", running: "Running", done: "Done", failed: "Failed" };
const TONES: Record<PostmortemStatus, string> = {
  owed: "bg-surface-2 text-muted",
  running: "bg-info-soft text-info",
  done: "bg-ok-soft text-ok",
  failed: "bg-danger-soft text-danger",
};
export const BUTTON =
  "inline-flex min-h-7 items-center justify-center rounded border border-border bg-surface px-2.5 text-xs text-text hover:border-border-strong disabled:cursor-not-allowed disabled:opacity-50";
export function Time({ ms }: { ms: number }) {
  return (
    <time className="font-mono text-2xs text-faint" dateTime={new Date(ms).toISOString()}>
      {new Date(ms).toLocaleString("en-GB", { timeZone: "UTC", dateStyle: "medium", timeStyle: "short" })} UTC
    </time>
  );
}
export function Selection({ item }: { item: Postmortem }) {
  return (
    <>
      {REASONS[item.reason]}
      {item.sample_share !== null && ` · ${Math.round(item.sample_share * 100)}% sample share`}
    </>
  );
}
export function Status({ item }: { item: Postmortem }) {
  return (
    <span className={`inline-flex rounded px-1.5 py-0.5 text-2xs font-medium ${TONES[item.status]}`}>
      {STATUS[item.status]}
    </span>
  );
}
/** Judge JSON stays readable while slice B settles its nested fields. */
function Findings({ value }: { value: unknown }) {
  if (value === null || value === undefined) return null;
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean")
    return <span className="whitespace-pre-wrap break-words">{String(value)}</span>;
  if (Array.isArray(value))
    return (
      <ol className="flex flex-col gap-2">
        {value.map((item: unknown, index) => (
          <li key={index}>
            <Findings value={item} />
          </li>
        ))}
      </ol>
    );
  return (
    <dl className="grid gap-1">
      {Object.entries(value).map(([key, child]: [string, unknown]) => (
        <div key={key}>
          <dt className="text-2xs text-faint">{key.replaceAll("_", " ")}</dt>
          <dd>
            <Findings value={child} />
          </dd>
        </div>
      ))}
    </dl>
  );
}
function hasFindings(item: Postmortem) {
  return Boolean(
    item.summary ||
      item.cause ||
      item.notes ||
      (item.timeline && JSON.stringify(item.timeline) !== "[]") ||
      (item.process_failures && JSON.stringify(item.process_failures) !== "[]") ||
      item.recommendations.length,
  );
}
function Report({ item }: { item: Postmortem }) {
  return (
    <article aria-label={`Postmortem ${item.trigger}`} className="rounded-md border border-border bg-surface p-3">
      <div className="flex flex-wrap items-center gap-2">
        <Status item={item} />
        <Time ms={item.failed_ms ?? item.finished_ms ?? item.started_ms ?? item.owed_ms} />
        <span className="font-mono text-2xs text-faint">{item.trigger}</span>
      </div>
      <p className="mt-2 text-xs font-medium text-text">
        <Selection item={item} />
      </p>
      <dl className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-2xs text-faint">
        {(
          [
            ["Owed", item.owed_ms],
            ["Started", item.started_ms],
            ["Finished", item.finished_ms],
            ["Failed", item.failed_ms],
          ] as const
        ).map(
          ([label, ms]) =>
            ms !== null && (
              <div key={label}>
                <dt className="inline">{label} </dt>
                <dd className="inline">
                  <Time ms={ms} />
                </dd>
              </div>
            ),
        )}
      </dl>
      {item.failure && (
        <p role="alert" className="mt-2 text-xs whitespace-pre-wrap text-danger">
          {item.failure}
        </p>
      )}
      {!hasFindings(item) ? (
        <p className="mt-3 rounded bg-surface-2 px-3 py-2 text-xs text-muted">
          Judge's findings arrive when the postmortem runs
        </p>
      ) : (
        <div className="mt-3 space-y-3 text-xs text-text">
          {item.summary && (
            <section aria-label="What went wrong">
              <h4 className="mb-1 text-2xs font-semibold text-muted">What went wrong</h4>
              <p className="whitespace-pre-wrap">{item.summary}</p>
            </section>
          )}
          {item.cause && (
            <p>
              <span className="text-muted">Cause · </span>
              {item.cause === "tl3" ? "TL3" : "Outside TL3"}
            </p>
          )}
          {item.timeline !== null && (
            <section aria-label="Postmortem timeline">
              <h4 className="mb-1 text-2xs font-semibold text-muted">Timeline</h4>
              <Findings value={item.timeline} />
            </section>
          )}
          {item.process_failures !== null && (
            <section aria-label="Process failures">
              <h4 className="mb-1 text-2xs font-semibold text-muted">Process failures</h4>
              <Findings value={item.process_failures} />
            </section>
          )}
          {item.notes && <p className="whitespace-pre-wrap text-muted">{item.notes}</p>}
        </div>
      )}
      {item.recommendations.length > 0 && (
        <section aria-label="Recommendations" className="mt-3">
          <h4 className="text-2xs font-semibold text-muted">Recommendations</h4>
          <ol className="mt-2 space-y-2">
            {item.recommendations.slice(0, 3).map((rec) => (
              <li key={rec.id} className="rounded border border-border p-2 text-xs">
                <p className="font-medium text-text">{rec.target}</p>
                <p className="mt-0.5 text-2xs text-muted">
                  Seen in {rec.seen_in} {rec.seen_in === 1 ? "postmortem" : "postmortems"}
                </p>
                <p className="mt-1 whitespace-pre-wrap text-text">{rec.effect}</p>
                <dl className="mt-2 space-y-1 text-muted">
                  <div>
                    <dt className="text-2xs text-faint">Evidence</dt>
                    <dd className="whitespace-pre-wrap">{rec.evidence}</dd>
                  </div>
                  {rec.before && (
                    <div>
                      <dt className="text-2xs text-faint">Before</dt>
                      <dd className="whitespace-pre-wrap">{rec.before}</dd>
                    </div>
                  )}
                  <div>
                    <dt className="text-2xs text-faint">{rec.operation === "remove" ? "Remove" : "Proposed change"}</dt>
                    <dd className="whitespace-pre-wrap">{rec.after || rec.before}</dd>
                  </div>
                </dl>
              </li>
            ))}
          </ol>
        </section>
      )}
      {item.grades !== null && (
        <details className="mt-3 text-xs text-muted">
          <summary className="cursor-pointer">Judge's grades</summary>
          <Findings value={item.grades} />
        </details>
      )}
    </article>
  );
}
export function PostmortemTab({ id }: { id: number }) {
  const query = usePostmortems(id);
  const reports = [
    ...new Map(query.data?.pages.flatMap((page) => page.postmortems).map((item) => [item.id, item])).values(),
  ];
  const grades = [
    ...new Map(query.data?.pages.flatMap((page) => page.grades).map((grade) => [grade.id, grade])).values(),
  ];
  return (
    <div className="space-y-3 border-t border-border p-4">
      {query.error && (
        <p role="alert" className="text-xs text-danger">
          {query.error.message}{" "}
          <button className={BUTTON} onClick={() => void query.refetch()}>
            Retry
          </button>
        </p>
      )}
      {query.isPending && (
        <p role="status" className="text-xs text-muted">
          Reading postmortems…
        </p>
      )}
      {query.data && reports.length === 0 && (
        <p className="text-xs text-muted">
          No postmortem recorded for this request. TL3 selects fixes, retries and escalations, plus a share of clean
          work. Rob can ask for one here.
        </p>
      )}
      {reports.map((item) => (
        <Report key={item.id} item={item} />
      ))}
      {grades.length > 0 && (
        <section aria-label="Code grades and recorded facts" className="rounded-md border border-border p-3">
          <h3 className="text-xs font-semibold">Code grades and recorded facts</h3>
          <p className="mt-1 text-2xs text-faint">
            Recorded by TL3 from the request's history. Grades run from 1 to 5.
          </p>
          <ul className="mt-2 divide-y divide-border">
            {grades.map((grade) => (
              <li key={grade.id} className="py-2 text-xs">
                <div className="flex items-baseline justify-between gap-3">
                  <span className="font-medium capitalize">{grade.dimension.replaceAll("_", " ")}</span>
                  <span className="font-mono">{grade.grade === null ? "Recorded fact" : `${grade.grade} / 5`}</span>
                </div>
                <p className="mt-0.5 text-2xs text-faint">
                  {grade.trigger} · rubric {grade.rubric_version} · <Time ms={grade.recorded_ms} />
                </p>
                <pre className="mt-1 overflow-auto rounded bg-surface-2 px-2 py-1 font-mono text-2xs whitespace-pre-wrap text-muted">
                  {JSON.stringify(grade.fact, null, 2)}
                </pre>
              </li>
            ))}
          </ul>
        </section>
      )}
      {query.hasNextPage && (
        <button className={BUTTON} disabled={query.isFetchingNextPage} onClick={() => void query.fetchNextPage()}>
          Load older postmortems and facts
        </button>
      )}
    </div>
  );
}
