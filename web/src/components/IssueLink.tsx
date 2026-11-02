export function issueUrl(_repo: string, _issue: number): string | null {
  return null;
}

export function IssueLink({ repo, issue, short = false }: { repo: string; issue: number; short?: boolean }) {
  return (
    <span>
      <span>{short ? (repo.split("/")[1] ?? repo) : repo}</span>
      <span>#{issue}</span>
    </span>
  );
}
