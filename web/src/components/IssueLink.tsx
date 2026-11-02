/** GitHub's own rule for owner and repository names, bar "." and "..". */
const NAME = /^[A-Za-z0-9_.-]+$/;

/**
 * The issue's GitHub page, or null when `repo` isn't `owner/name`. This is the
 * one place the web app builds the URL; the case file's own link comes from
 * the console, and a test keeps the two equal.
 */
export function issueUrl(repo: string, issue: number): string | null {
  const [owner, name, ...rest] = repo.split("/");
  const valid = (part: string | undefined): part is string =>
    part !== undefined && NAME.test(part) && part !== "." && part !== "..";
  if (rest.length > 0 || !valid(owner) || !valid(name) || !Number.isSafeInteger(issue) || issue < 0) return null;
  return `https://github.com/${owner}/${name}/issues/${issue}`;
}

/**
 * `owner/name#123` (or `name#123` when `short`) as a link to the issue on
 * GitHub, in a new tab. It is a plain anchor, so it sits beside the card's own
 * link and never inside a button; a click on it does nothing else.
 */
export function IssueLink({
  repo,
  issue,
  short = false,
  className = "",
}: {
  repo: string;
  issue: number;
  short?: boolean;
  className?: string;
}) {
  const url = issueUrl(repo, issue);
  const name = short ? (repo.split("/")[1] ?? repo) : repo;
  if (url === null) {
    return (
      <span className={`inline-flex min-w-0 items-baseline gap-1 ${className}`}>
        <span className="truncate">{name}</span>
        <span className="font-mono text-faint">#{issue}</span>
      </span>
    );
  }
  return (
    <a
      href={url}
      target="_blank"
      rel="noreferrer"
      title={`Open ${repo}#${issue} on GitHub`}
      // Browsers join flex items' text with a space; the name is the visible text.
      aria-label={`${name}#${issue}`}
      className={`group/issue inline-flex min-w-0 items-baseline gap-1 rounded-sm ${className}`}
    >
      <span className="truncate underline-offset-2 group-hover/issue:text-text group-hover/issue:underline">{name}</span>
      <span className="font-mono text-faint underline-offset-2 group-hover/issue:text-text group-hover/issue:underline">
        #{issue}
      </span>
    </a>
  );
}
