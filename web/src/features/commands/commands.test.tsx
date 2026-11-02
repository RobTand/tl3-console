import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import type { Action, CardQuestion, CommandRecord } from "../../api/types.gen";
import { card } from "../../fixtures";
import { commandId, fileCommand } from "./api";
import { CardCommands } from "./CardCommands";
import { CommandsProvider } from "./commands";
import { DeferAll } from "./DeferAll";

type Filed = { node: number; id: string; action: Action };

function setup(children: ReactNode, records: CommandRecord[] = []) {
  const filed: Filed[] = [];
  const file = vi.fn(async (node: number, id: string, action: Action) => {
    filed.push({ node, id, action });
    return { id, filed: true };
  });
  const view = render(
    <CommandsProvider records={records} file={file}>
      {children}
    </CommandsProvider>,
  );
  const rerender = (next: CommandRecord[]) =>
    view.rerender(
      <CommandsProvider records={next} file={file}>
        {children}
      </CommandsProvider>,
    );
  return { filed, rerender };
}

function record(id: string, node: number, action: Action, status: CommandRecord["status"], reason: string | null = null) {
  return { id, node, action, status, reason, received_ms: 1, settled_ms: status === "pending" ? null : 2 };
}

const EFFECT_FAILED: CardQuestion = {
  id: 9,
  prompt: "A required action needs your help before work can continue.",
  options: [
    {
      answer: "continue",
      label: "Continue after fixing the cause",
      outcome: "I will continue once you confirm the cause is fixed.",
      detail: null,
    },
    { answer: "stop", label: "Stop", outcome: "I will stop this work.", detail: null },
  ],
  recommended: 0,
  reason: "This option gives your request a clear next step.",
  defer_blocked: null,
};

const OUT_OF_CREDIT: CardQuestion = {
  id: 11,
  prompt: "This request has used its agreed allowance for further attempts.",
  options: [
    { answer: "add_credit", label: "Add credit", outcome: "I will continue with the extra allowance you provide.", detail: "amount" },
    { answer: "change_request", label: "Change the request", outcome: "I will stop this work and start from your new request.", detail: "text" },
    { answer: "stop", label: "Stop", outcome: "I will stop this work.", detail: null },
  ],
  recommended: 0,
  reason: "This option gives your request a clear next step.",
  defer_blocked: "TL3's recommended option, \"Add credit\", needs a credit amount from you.",
};

describe("commandId", () => {
  it("makes version 4 UUIDs without crypto.randomUUID, which plain HTTP lacks", () => {
    const id = commandId((bytes) => bytes.fill(0xff));
    expect(id).toBe("ffffffff-ffff-4fff-bfff-ffffffffffff");
    expect(commandId()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });
});

describe("fileCommand", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("posts the typed command with the console's header", async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify({ id: "x", filed: true }), { status: 202 }));
    vi.stubGlobal("fetch", fetch);
    await expect(fileCommand(3, "x", { kind: "pause" })).resolves.toEqual({ id: "x", filed: true });
    expect(fetch).toHaveBeenCalledWith("/api/requests/3/commands", {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-TL3-Console": "1" },
      body: JSON.stringify({ id: "x", action: { kind: "pause" } }),
    });
  });

  it("reports the console's refusal", async () => {
    vi.stubGlobal("fetch", async () => new Response("Commands come only from the console's own page.\n", { status: 403 }));
    await expect(fileCommand(3, "x", { kind: "pause" })).rejects.toThrow("Commands come only from the console's own page.");
  });
});

describe("CardCommands", () => {
  it("confirms a pause in one step, then shows it sending, pending and applied", async () => {
    const user = userEvent.setup();
    const making = card({ id: 4 });
    const { filed, rerender } = setup(<CardCommands card={making} />);
    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(filed).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Confirm pause" }));
    expect(filed).toEqual([{ node: 4, id: filed[0]!.id, action: { kind: "pause" } }]);
    expect(screen.getByText("Pause: sending")).toBeVisible();
    act(() => rerender([record(filed[0]!.id, 4, { kind: "pause" }, "pending")]));
    expect(screen.getByText("Pause: pending")).toBeVisible();
    act(() => rerender([record(filed[0]!.id, 4, { kind: "pause" }, "applied")]));
    expect(screen.getByText("Pause: applied")).toBeVisible();
  });

  it("shows a refusal's reason, and cancel files nothing", async () => {
    const user = userEvent.setup();
    const { filed, rerender } = setup(<CardCommands card={card({ id: 4, badges: ["Paused"] })} />);
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(filed).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Resume" }));
    await user.click(screen.getByRole("button", { name: "Confirm resume" }));
    act(() => rerender([record(filed[0]!.id, 4, { kind: "resume" }, "refused", "The request isn't paused.")]));
    expect(screen.getByText("Resume refused: The request isn't paused.")).toBeVisible();
  });

  it("offers only restart on a stopped request", () => {
    setup(<CardCommands card={card({ id: 4, column: "Done", ending: "Stopped" })} />);
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["Restart"]);
  });

  it("offers nothing on a merged request", () => {
    setup(<CardCommands card={card({ id: 5, column: "Done", ending: "Merged" })} />);
    expect(screen.queryAllByRole("button")).toEqual([]);
  });

  it("clears an incident", async () => {
    const user = userEvent.setup();
    const { filed } = setup(
      <CardCommands card={card({ id: 4, incidents: [{ id: "0.workers.transport", summary: "Checks stalled" }] })} />,
    );
    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(filed[0]!.action).toEqual({ kind: "clear_incident", incident: "0.workers.transport" });
  });

  it("shows a question's options with the recommendation, and lets TL3 decide", async () => {
    const user = userEvent.setup();
    const { filed } = setup(<CardCommands card={card({ id: 4, column: "NeedsRob", question: EFFECT_FAILED })} />);
    const panel = screen.getByRole("region", { name: "Question 9" });
    expect(within(panel).getByText(EFFECT_FAILED.prompt, { exact: false })).toBeVisible();
    const options = within(within(panel).getByRole("list", { name: "Options" })).getAllByRole("button");
    expect(options.map((option) => option.textContent)).toEqual(["Continue after fixing the causeRecommended", "Stop"]);
    expect(within(panel).getByText(/TL3 recommends “Continue after fixing the cause”/)).toBeVisible();
    await user.click(within(panel).getByRole("button", { name: "Let TL3 decide" }));
    expect(filed[0]!.action).toEqual({ kind: "defer", question: 9 });
    await user.click(options[1]!);
    expect(filed[1]!.action).toEqual({ kind: "answer", question: 9, choice: { answer: "stop" } });
  });

  it("asks for an option's details, and can't defer when the recommendation needs them", async () => {
    const user = userEvent.setup();
    const { filed } = setup(<CardCommands card={card({ id: 4, column: "NeedsRob", question: OUT_OF_CREDIT })} />);
    const decide = screen.getByRole("button", { name: "Let TL3 decide" });
    expect(decide).toBeDisabled();
    expect(screen.getByText(OUT_OF_CREDIT.defer_blocked!)).toBeVisible();
    await user.click(screen.getByRole("button", { name: /^Add credit/ }));
    const send = screen.getByRole("button", { name: "Send add credit" });
    expect(send).toBeDisabled();
    await user.type(screen.getByLabelText("Credit (1–100)"), "20");
    await user.click(send);
    expect(filed[0]!.action).toEqual({ kind: "answer", question: 11, choice: { answer: "add_credit", amount: 20 } });
  });

  it("sends Rob's own words to the classifier", async () => {
    const user = userEvent.setup();
    const { filed } = setup(<CardCommands card={card({ id: 4, column: "NeedsRob", question: EFFECT_FAILED })} />);
    await user.click(screen.getByText("Write my own answer"));
    await user.type(screen.getByLabelText("Your answer"), "Retry after the mirror is back");
    await user.click(screen.getByRole("button", { name: "Send answer" }));
    expect(filed[0]!.action).toEqual({ kind: "comment", text: "Retry after the mirror is back" });
  });
});

describe("DeferAll", () => {
  it("lists each question's choice before applying, and skips ones TL3 can't decide alone", async () => {
    const user = userEvent.setup();
    const cards = [
      card({ id: 1, issue: 101, column: "NeedsRob", question: EFFECT_FAILED, title: "Fix the build" }),
      card({ id: 2, issue: 102, column: "NeedsRob", question: OUT_OF_CREDIT, title: "Add caching" }),
      card({ id: 3, issue: 103, column: "NeedsRob", question: { ...EFFECT_FAILED, id: 12 }, title: "Tidy docs" }),
    ];
    const { filed } = setup(<DeferAll cards={cards} />);
    await user.click(screen.getByRole("button", { name: "Let TL3 decide on all" }));
    const dialog = screen.getByRole("dialog", { name: "Let TL3 decide on all" });
    const items = within(within(dialog).getByRole("list", { name: "Choices" })).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "tl3-sandbox#101 Fix the buildQuestion 9: Continue after fixing the cause",
      `tl3-sandbox#102 Add cachingSkipped: ${OUT_OF_CREDIT.defer_blocked}`,
      "tl3-sandbox#103 Tidy docsQuestion 12: Continue after fixing the cause",
    ]);
    expect(filed).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Apply 2 choices" }));
    expect(filed.map(({ node, action }) => ({ node, action }))).toEqual([
      { node: 1, action: { kind: "defer", question: 9 } },
      { node: 3, action: { kind: "defer", question: 12 } },
    ]);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
