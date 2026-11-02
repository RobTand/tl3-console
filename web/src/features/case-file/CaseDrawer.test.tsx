import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { card } from "../../fixtures";
import { CaseDrawer, CaseFileView } from "./CaseDrawer";
import { caseFile, run, T0 } from "./fixtures";

const MINUTE = 60_000;

function section(name: string) {
  return within(screen.getByRole("region", { name }));
}

describe("CaseFileView", () => {
  it("names the request and links to GitHub by accessible name", () => {
    render(<CaseFileView file={caseFile()} now={T0} />);
    expect(screen.getByRole("heading", { name: "Cache compiled kernels between campaign rows" })).toBeVisible();
    const issue = screen.getByRole("link", { name: "Issue RobTand/tl3-sandbox#102 on GitHub" });
    expect(issue).toHaveAttribute("href", "https://github.com/RobTand/tl3-sandbox/issues/102");
    expect(issue).toHaveAttribute("rel", "noreferrer");
    expect(screen.getByRole("link", { name: "Pull request #7 on GitHub" })).toBeVisible();
  });

  it("links the header's issue reference to the same page the case file's own link names", () => {
    render(<CaseFileView file={caseFile()} now={T0} />);
    const header = screen.getByRole("link", { name: "RobTand/tl3-sandbox#102" });
    const served = caseFile().links.find((link) => link.kind === "Issue")!;
    expect(header).toHaveAttribute("href", served.url);
    expect(header).toHaveAttribute("target", "_blank");
    expect(header).toHaveAttribute("rel", "noreferrer");
  });

  it("names the column and ending of cancelled and stopped requests", () => {
    const { rerender } = render(
      <CaseFileView
        now={T0}
        file={caseFile({ card: card({ id: 1, column: "Cancelled", ending: "Duplicate", phase: "Closed: duplicate" }) })}
      />,
    );
    expect(screen.getByText("Cancelled")).toBeVisible();
    expect(screen.getByText("Duplicate")).toBeVisible();
    rerender(
      <CaseFileView
        now={T0}
        file={caseFile({
          card: card({ id: 1, column: "Backlog", ending: "Stopped", phase: "Closed: stopped", badges: ["Stopped"] }),
        })}
      />,
    );
    expect(screen.getByText("Backlog")).toBeVisible();
    expect(screen.getByText("Stopped")).toBeVisible();
  });

  it("counts attempts, rounds, retries, failures and credit", () => {
    render(
      <CaseFileView
        now={T0}
        file={caseFile({
          card: card({
            id: 1,
            attempt: 2,
            round: 1,
            credit: 1,
            retries: { plan: 1, audit: 2, run_failures: 4, effect_failures: 5 },
          }),
        })}
      />,
    );
    const counters = section("Counters");
    for (const [term, value] of [
      ["Attempt", "2"],
      ["In a fix round", "Yes"],
      ["Plan retries", "1"],
      ["Audit rounds", "2"],
      ["Run failures", "4"],
      ["Effect failures", "5"],
      ["Credit", "1"],
    ]) {
      expect(counters.getByText(term!).nextElementSibling).toHaveTextContent(value!);
    }
  });

  it("lists every stage change in order with its time, stage and cause", () => {
    render(<CaseFileView file={caseFile()} now={T0} />);
    const items = section("Timeline").getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent("Intake");
    expect(items[0]).toHaveTextContent("Rob labelled the issue");
    expect(items[2]).toHaveTextContent("Making");
    expect(items[2]).toHaveTextContent("The planner returned a plan");
    expect(within(items[1]!).getByText(/09:04:00/)).toBeVisible();
  });

  it("shows each run's role, model, duration, tokens, outcome and trace link", () => {
    render(<CaseFileView file={caseFile()} now={T0 + 20 * MINUTE} />);
    const rows = section("Worker runs").getAllByRole("row").slice(1);
    expect(rows).toHaveLength(3);
    const first = within(rows[0]!);
    expect(first.getByText("Maker")).toBeVisible();
    expect(first.getByText("default-model · high")).toBeVisible();
    expect(first.getByText("12m")).toBeVisible();
    expect(first.getByText("Blocked by the environment")).toBeVisible();
    expect(first.getByTitle("Not recorded yet; see the trace")).toHaveTextContent("–");
    expect(first.getByRole("link", { name: "MLflow traces for the Maker run effect:1:4" })).toHaveAttribute(
      "href",
      "http://mlflow.test/#/experiments/2/traces?filter=tag%3A%3A%3D%3A%3A1%3A%3Arequest",
    );
    const review = within(rows[1]!);
    expect(review.getByText("alternate-model")).toBeVisible();
    expect(review.queryByRole("link")).toBeNull();
    const running = within(rows[2]!);
    expect(running.getByText("20m")).toBeVisible();
    expect(running.getByText("Running")).toBeVisible();
  });

  it("puts open incidents first and their evidence behind a disclosure", async () => {
    render(<CaseFileView file={caseFile()} now={T0} />);
    const items = section("Incidents").getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("Open");
    expect(items[0]).toHaveTextContent("Outside");
    expect(items[0]).toHaveTextContent("GitHub refused the pull request update.");
    expect(items[1]).toHaveTextContent("Cleared");
    expect(items[1]).toHaveTextContent("TL3");
    const evidence = within(items[0]!).getByText("http_502");
    expect(evidence).not.toBeVisible();
    await userEvent.click(within(items[0]!).getByText("Evidence"));
    expect(evidence).toBeVisible();
    expect(within(items[0]!).getByText("effect:1:14")).toBeVisible();
  });

  it("shows a maker's blocked report with its evidence, and its own words on request", async () => {
    render(<CaseFileView file={caseFile()} now={T0} />);
    const blocked = section("Maker blocked");
    expect(blocked.getByText("The maker's write tool recorded tool-error-1 (EROFS)")).toBeVisible();
    const report = blocked.getByText("The worktree is mounted read-only, so no file could be written.");
    expect(report).not.toBeVisible();
    await userEvent.click(blocked.getByText("Maker's report"));
    expect(report).toBeVisible();
  });

  it("shows the pending question read-only, with TL3's recommendation marked", () => {
    render(
      <CaseFileView
        now={T0}
        file={caseFile({
          question: {
            id: 4,
            kind: "Can't meet the request",
            obstacle: "I could not finish your request with the approach I tried.",
            options: [
              { action: "Try again with guidance", outcome: "I will try again using the guidance you provide." },
              { action: "Change the request", outcome: "I will stop this work and start from your new request." },
              { action: "Stop", outcome: "I will stop this work." },
            ],
            recommended_option: 0,
            recommendation_reason: "This option gives your request a clear next step.",
            how_to_answer: "Reply on the issue, naming question 4.",
            posted: true,
          },
        })}
      />,
    );
    const question = section("Question for Rob");
    expect(question.getByText("I could not finish your request with the approach I tried.")).toBeVisible();
    const options = question.getAllByRole("listitem");
    expect(options).toHaveLength(3);
    expect(options[0]).toHaveTextContent("Try again with guidance");
    expect(options[0]).toHaveTextContent("Recommended");
    expect(options[1]).not.toHaveTextContent("Recommended");
    expect(question.getByText("This option gives your request a clear next step.")).toBeVisible();
    expect(question.queryAllByRole("button")).toHaveLength(0);
  });

  it("lists proposed pieces and replacement requests as child items", () => {
    render(
      <CaseFileView
        now={T0}
        file={caseFile({
          children: [
            { id: null, title: "Move GPU hosts into their own fleet file", column: null, stage: "Proposed", files: ["fleet/gpu.toml"] },
            { id: 9, title: "Split the fleet file", column: "Making", stage: "Making the change", files: [] },
          ],
        })}
      />,
    );
    const items = section("Child items").getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("Move GPU hosts into their own fleet file");
    expect(items[0]).toHaveTextContent("Proposed");
    expect(items[0]).toHaveTextContent("fleet/gpu.toml");
    expect(items[1]).toHaveTextContent("Making the change");
  });

  it("shows the closing summary of a closed request", () => {
    render(
      <CaseFileView
        now={T0}
        file={caseFile({
          summary: {
            ending: "Stopped",
            outcome: "Work on this request has stopped.",
            unmet_goals: ["The original request was not completed by this work."],
            removed_changes: null,
            note: null,
          },
        })}
      />,
    );
    const summary = section("Closing summary");
    expect(summary.getByText("Stopped")).toBeVisible();
    expect(summary.getByText("Work on this request has stopped.")).toBeVisible();
    expect(summary.getByText("The original request was not completed by this work.")).toBeVisible();
  });

  it("names the drawer by the request's visible title, shown once", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(caseFile()), { status: 200 })));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <CaseDrawer id={1} card={undefined} now={T0} onClose={() => {}} />
      </QueryClientProvider>,
    );
    const dialog = await screen.findByRole("dialog", { name: "Cache compiled kernels between campaign rows" });
    expect(within(dialog).getAllByText("Cache compiled kernels between campaign rows")).toHaveLength(1);
    vi.unstubAllGlobals();
  });

  it("leaves out empty sections", () => {
    render(<CaseFileView file={caseFile({ incidents: [], blocked: [], runs: [run({ key: "effect:1:1" })] })} now={T0} />);
    expect(screen.queryByRole("region", { name: "Incidents" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Maker blocked" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Question for Rob" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Child items" })).toBeNull();
  });
});
