import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CaseDrawer } from "../case-file/CaseDrawer";
import { caseFile, T0 } from "../case-file/fixtures";
import { CardCommands, CommandsProvider } from "../commands";
import { card } from "../../fixtures";

const postmortem = {
  id: 3, node_id: "1", trigger: "question:8", reason: "sampled", sample_share: 0.1,
  repo: "RobTand/tl3-sandbox", issue: 102, title: "Kernel cache", status: "owed",
  owed_ms: T0, started_ms: null, finished_ms: null, failed_ms: null, failure: null,
  cause: null, summary: null, timeline: null, process_failures: null, notes: null, grades: null,
  run_key: null, model: null, rubric_version: null, recommendations: [],
};
function setup(status = "owed") {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => new Response(JSON.stringify(
    url.includes("postmortems")
      ? { postmortems: [{ ...postmortem, status }], grades: [{ id: 1, node_id: "1", trigger: "close", rubric_version: 1, dimension: "reliability", grade: 4, fact: { fix_rounds: 1 }, recorded_ms: T0 }], next_before: null, next_grade_before: null, active: status === "owed" || status === "running" }
      : caseFile(),
  ))));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { client, wrap: ({ children }: { children: React.ReactNode }) => <QueryClientProvider client={client}><CommandsProvider records={[]}>{children}</CommandsProvider></QueryClientProvider> };
}

afterEach(() => vi.unstubAllGlobals());

it("opens a Postmortem tab with the selection, pending judge state and code facts", async () => {
  const { wrap } = setup();
  render(<CaseDrawer id={1} card={undefined} now={T0} onClose={() => {}} />, { wrapper: wrap });
  const user = userEvent.setup();
  await user.click(await screen.findByRole("tab", { name: "Postmortem" }));
  expect(await screen.findByText("Judge's findings arrive when the postmortem runs")).toBeVisible();
  expect(screen.getByText(/Sampled.*10%/)).toBeVisible();
  expect(screen.getByText("4 / 5")).toBeVisible();
  expect(screen.getByText(/fix_rounds/)).toBeVisible();
});

it("disables Rob's button with the owed or running reason", async () => {
  const { wrap } = setup();
  render(<CardCommands card={card({ id: 1, column: "Done", ending: "Merged" })} />, { wrapper: wrap });
  expect(await screen.findByRole("button", { name: "Write postmortem" })).toBeDisabled();
  expect(await screen.findByText(/already owed or running/)).toBeVisible();
});

it("offers one confirmation on Needs Rob and files the typed command", async () => {
  const { client } = setup("done");
  const file = vi.fn(async (_node: number, id: string) => ({ id, filed: true }));
  render(<QueryClientProvider client={client}><CommandsProvider records={[]} file={file}>
    <CardCommands card={card({ id: 1, column: "NeedsRob" })} />
  </CommandsProvider></QueryClientProvider>);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Write postmortem" }));
  expect(file).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Confirm postmortem" }));
  expect(file).toHaveBeenCalledWith(1, expect.any(String), { kind: "postmortem" });
});
