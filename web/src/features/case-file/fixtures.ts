import type { CaseFile, WorkerRun } from "../../api/types.gen";
import { card } from "../../fixtures";

const MINUTE = 60_000;
export const T0 = Date.UTC(2026, 10, 2, 9, 0);

export function run(fields: Partial<WorkerRun> & { key: string }): WorkerRun {
  return {
    role: "Maker",
    generation: 7,
    attempt: 1,
    round: 1,
    model: "default-model",
    effort: "high",
    started_ms: T0,
    ended_ms: T0 + 12 * MINUTE,
    tokens: null,
    status: "Finished",
    outcome: "Made a change",
    trace_url: "http://mlflow.test/#/experiments/2/traces?filter=tag%3A%3A%3D%3A%3A1%3A%3Arequest",
    ...fields,
  };
}

export function caseFile(fields: Partial<CaseFile> = {}): CaseFile {
  return {
    revision: 40,
    card: card({ id: 1, issue: 102, title: "Cache compiled kernels between campaign rows", round: 1 }),
    timeline: [
      { revision: 3, at_ms: T0, column: "Intake", step: "Reader started", cause: "Rob labelled the issue" },
      { revision: 9, at_ms: T0 + 4 * MINUTE, column: "Planning", step: "Planner started", cause: "The auditor agreed" },
      { revision: 15, at_ms: T0 + 9 * MINUTE, column: "Making", step: "Maker started", cause: "The planner returned a plan" },
    ],
    children: [],
    runs: [
      run({ key: "effect:1:4", role: "Maker", status: "Failed", outcome: "Blocked by the environment", round: 0 }),
      run({
        key: "effect:1:9",
        role: "Reviewer",
        model: "alternate-model",
        effort: null,
        outcome: "Changes requested",
        trace_url: null,
      }),
      run({ key: "effect:1:12", role: "Maker", status: "Running", ended_ms: null, outcome: "Running", round: 1 }),
    ],
    incidents: [
      {
        id: "incident-12",
        summary: "The maker could not write to its worktree. Evidence: the maker's write tool recorded tool-error-1.",
        open: false,
        cause: "Tl3",
        needs_rob: true,
        blocking: true,
        source: "Workers",
        class: "environment_blocked",
        effect: null,
      },
      {
        id: "incident-20",
        summary: "GitHub refused the pull request update.",
        open: true,
        cause: "Outside",
        needs_rob: false,
        blocking: true,
        source: "GitHub",
        class: "http_502",
        effect: "effect:1:14",
      },
    ],
    blocked: [
      {
        generation: 4,
        evidence: "The maker's write tool recorded tool-error-1 (EROFS)",
        report: "The worktree is mounted read-only, so no file could be written.",
      },
    ],
    question: null,
    summary: null,
    links: [
      { kind: "Issue", label: "Issue RobTand/tl3-sandbox#102 on GitHub", url: "https://github.com/RobTand/tl3-sandbox/issues/102" },
      { kind: "PullRequest", label: "Pull request #7 on GitHub", url: "https://github.com/RobTand/tl3-sandbox/pull/7" },
    ],
    ...fields,
  };
}
