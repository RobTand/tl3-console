import { useState } from "react";
import { usePostmortemList } from "./api";
import { BUTTON, REASONS, Selection, Status, Time } from "./PostmortemTab";

const INPUT = "h-8 min-w-0 rounded border border-border bg-surface px-2 text-xs text-text";
export function PostmortemsPage() {
  const [filters, setFilters] = useState("");
  const query = usePostmortemList(filters);
  const reports = query.data?.pages.flatMap((page) => page.postmortems) ?? [];
  return (
    <main className="flex min-h-0 flex-1 flex-col rounded-md border border-border bg-surface">
      <header className="border-b border-border p-3">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h2 className="text-base font-semibold">Postmortems</h2>
          <p className="text-xs text-muted">Fixes, retries, escalations and a share of clean work.</p>
        </div>
        <form
          className="mt-3 flex flex-wrap items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            const params = new URLSearchParams();
            for (const [key, value] of new FormData(event.currentTarget))
              if (typeof value === "string" && value.trim()) params.set(key, value.trim());
            setFilters(params.toString());
          }}
        >
          <label className="flex min-w-44 flex-1 flex-col gap-1 text-2xs text-muted">
            Search postmortems
            <input
              name="q"
              type="search"
              placeholder="Findings or recommendation evidence"
              className={INPUT}
              maxLength={500}
            />
          </label>
          <label className="flex flex-col gap-1 text-2xs text-muted">
            Repository
            <input name="repo" placeholder="owner/repo" className={`${INPUT} w-40`} maxLength={200} />
          </label>
          <label className="flex flex-col gap-1 text-2xs text-muted">
            Cause
            <select name="cause" className={INPUT}>
              <option value="">Any cause</option>
              <option value="tl3">TL3</option>
              <option value="outside">Outside TL3</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-2xs text-muted">
            Reason
            <select name="reason" className={INPUT}>
              <option value="">Any reason</option>
              {Object.entries(REASONS).map(([value, text]) => (
                <option key={value} value={value}>
                  {text}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-2xs text-muted">
            Status
            <select name="status" className={INPUT}>
              <option value="">Any status</option>
              <option value="owed">Owed</option>
              <option value="running">Running</option>
              <option value="done">Done</option>
              <option value="failed">Failed</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-2xs text-muted">
            From
            <input type="date" name="from" className={INPUT} />
          </label>
          <label className="flex flex-col gap-1 text-2xs text-muted">
            Through
            <input type="date" name="to" className={INPUT} />
          </label>
          <button className={`${BUTTON} h-8`} type="submit">
            Search
          </button>
          <button className={`${BUTTON} h-8`} type="reset" onClick={() => setFilters("")}>
            Reset
          </button>
        </form>
      </header>
      {query.error && (
        <p role="alert" className="m-3 text-xs text-danger">
          {query.error.message}{" "}
          <button className={BUTTON} onClick={() => void query.refetch()}>
            Retry
          </button>
        </p>
      )}
      {query.isPending && (
        <p role="status" className="p-4 text-xs text-muted">
          Reading postmortems…
        </p>
      )}
      {!query.isPending && !query.error && reports.length === 0 && (
        <p className="p-4 text-xs text-muted">No postmortems match these filters.</p>
      )}
      <div className="min-h-0 flex-1 overflow-auto">
        <table className="w-full text-left text-xs">
          <caption className="sr-only">Postmortems, newest first</caption>
          <thead className="sticky top-0 bg-surface-2 text-2xs text-muted">
            <tr>
              {["Request", "Status", "Selection", "Owed", "Recommendations"].map((label) => (
                <th className="px-3 py-2 font-medium" key={label}>
                  {label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {reports.map((item) => (
              <tr key={item.id} className="align-top hover:bg-surface-2/50">
                <td className="min-w-48 max-w-sm px-3 py-2">
                  <a
                    className="font-medium text-text underline-offset-2 hover:text-accent hover:underline"
                    href={`?request=${encodeURIComponent(item.node_id)}&tab=postmortem#/postmortems`}
                    onClick={(event) => {
                      if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey)
                        return;
                      event.preventDefault();
                      window.history.pushState(null, "", event.currentTarget.href);
                      window.dispatchEvent(new PopStateEvent("popstate"));
                    }}
                  >
                    {item.title}
                  </a>
                  <p className="mt-0.5 text-2xs text-faint">
                    {item.repo}#{item.issue} · {item.trigger}
                  </p>
                  {item.summary && <p className="mt-1 line-clamp-2 text-muted">{item.summary}</p>}
                </td>
                <td className="px-3 py-2">
                  <Status item={item} />
                  {item.cause && (
                    <p className="mt-1 text-2xs text-faint">{item.cause === "tl3" ? "TL3" : "Outside TL3"}</p>
                  )}
                </td>
                <td className="px-3 py-2 text-muted">
                  <Selection item={item} />
                </td>
                <td className="px-3 py-2 whitespace-nowrap">
                  <Time ms={item.owed_ms} />
                </td>
                <td className="min-w-44 px-3 py-2 text-muted">
                  {item.recommendations.length ? (
                    <ul className="space-y-1">
                      {item.recommendations.map((rec) => (
                        <li key={rec.id}>
                          <span className="text-text">{rec.target}</span>
                          <span className="block text-2xs text-faint">
                            Seen in {rec.seen_in} {rec.seen_in === 1 ? "postmortem" : "postmortems"}
                          </span>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <span className="text-faint">
                      {item.status === "owed" || item.status === "running" ? "Awaiting findings" : "None recorded"}
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {query.hasNextPage && (
          <div className="p-3">
            <button className={BUTTON} disabled={query.isFetchingNextPage} onClick={() => void query.fetchNextPage()}>
              Load more postmortems
            </button>
          </div>
        )}
      </div>
    </main>
  );
}
