import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render as testingRender, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import type { Action, CardQuestion, CommandRecord } from "../../api/types.gen";
import { card } from "../../fixtures";
import { commandId, fileCommand } from "./api";
import { CardCommands } from "./CardCommands";
import { CommandsProvider } from "./commands";
import { DeferAll } from "./DeferAll";

function render(children: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return testingRender(children, { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
}

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
  decision: null,
  findings: null,
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
  decision: null,
  findings: null,
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
    expect(options).toHaveLength(2);
    expect(within(options[0]!).getByText("Continue after fixing the cause")).toBeVisible();
    expect(within(options[0]!).getByText("Recommended")).toBeVisible();
    expect(within(options[1]!).getByText("Stop")).toBeVisible();
    expect(within(options[1]!).queryByText("Recommended")).toBeNull();
    // Each option says what it does in visible text, not only in a tooltip.
    for (const [index, option] of EFFECT_FAILED.options.entries()) {
      expect(within(options[index]!).getByText(option.outcome)).toBeVisible();
      expect(options[index]).not.toHaveAttribute("title");
    }
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
    // The box clears and its button disables; focus stays in the box, not on the page.
    expect(screen.getByLabelText("Your answer")).toHaveValue("");
    expect(screen.getByLabelText("Your answer")).toHaveFocus();
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

describe("DeferAll links", () => {
  it("links each request to its issue on GitHub", async () => {
    const user = userEvent.setup();
    const cards = [
      card({ id: 1, issue: 101, column: "NeedsRob", question: EFFECT_FAILED, title: "Fix the build" }),
      card({ id: 2, issue: 7, repo: "RobTand/prismaquant", column: "NeedsRob", question: EFFECT_FAILED }),
    ];
    setup(<DeferAll cards={cards} />);
    await user.click(screen.getByRole("button", { name: "Let TL3 decide on all" }));
    const dialog = screen.getByRole("dialog", { name: "Let TL3 decide on all" });
    for (const { name, href } of [
      { name: "tl3-sandbox#101", href: "https://github.com/RobTand/tl3-sandbox/issues/101" },
      { name: "prismaquant#7", href: "https://github.com/RobTand/prismaquant/issues/7" },
    ]) {
      const link = within(dialog).getByRole("link", { name });
      expect(link).toHaveAttribute("href", href);
      expect(link).toHaveAttribute("target", "_blank");
    }
    expect(dialog.querySelector("button a")).toBeNull();
  });
});

describe("focus", () => {
  it("moves into the confirmation, and back to the action on cancel or confirm", async () => {
    const user = userEvent.setup();
    setup(<CardCommands card={card({ id: 4 })} />);
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.getByRole("button", { name: "Confirm stop" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Stop" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(screen.getByRole("button", { name: "Confirm pause" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("button", { name: "Pause" })).toHaveFocus();
  });

  it("moves into an option's form, and back to the option on cancel or send", async () => {
    const user = userEvent.setup();
    setup(<CardCommands card={card({ id: 4, column: "NeedsRob", question: OUT_OF_CREDIT })} />);
    const credit = () => screen.getByRole("button", { name: /^Add credit/ });
    await user.click(credit());
    expect(screen.getByLabelText("Credit (1–100)")).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(credit()).toHaveFocus();
    await user.click(credit());
    await user.keyboard("20{Enter}");
    expect(screen.queryByLabelText("Credit (1–100)")).toBeNull();
    expect(credit()).toHaveFocus();
  });
});

describe("compact cards", () => {
  it("keep their actions behind one menu that works from the keyboard", async () => {
    const user = userEvent.setup();
    const { filed } = setup(
      <CardCommands compact card={card({ id: 4, incidents: [{ id: "0.workers.transport", summary: "Checks stalled" }] })} />,
    );
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
    const menu = screen.getByRole("button", { name: "Actions" });
    menu.focus();
    await user.keyboard("{Enter}");
    const items = screen.getAllByRole("menuitem").map((item) => item.textContent);
    expect(items).toEqual(["Pause", "Stop", "Restart", "Clear incident 0.workers.transport"]);
    await user.keyboard("{Enter}");
    // The menu hands focus back once it has closed.
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm pause" })).toHaveFocus());
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(menu).toHaveFocus();
    expect(filed).toEqual([]);
    await user.keyboard("{Enter}");
    await user.keyboard("{ArrowDown}{Enter}");
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm stop" })).toHaveFocus());
    await user.keyboard("{Enter}");
    expect(filed.map((command) => command.action)).toEqual([{ kind: "stop" }]);
    expect(menu).toHaveFocus();
  });
});

describe("status", () => {
  it("mounts the live region empty, then fills it", async () => {
    const user = userEvent.setup();
    const { container } = render(
      <CommandsProvider records={[]} file={async (_node, id) => ({ id, filed: true })}>
        <CardCommands card={card({ id: 4 })} />
      </CommandsProvider>,
    );
    const live = container.querySelector("[aria-live]");
    expect(live).not.toBeNull();
    expect(live).toHaveTextContent("");
    await user.click(screen.getByRole("button", { name: "Pause" }));
    await user.click(screen.getByRole("button", { name: "Confirm pause" }));
    expect(container.querySelector("[aria-live]")).toBe(live);
    expect(live).toHaveTextContent("Pause: sending");
  });

  it("shows a request's newest settled outcome from the stream after a reload", () => {
    setup(<CardCommands card={card({ id: 4 })} />, [
      record("b", 4, { kind: "stop" }, "refused", "The request is already stopping."),
      record("a", 4, { kind: "pause" }, "applied"),
      record("c", 5, { kind: "pause" }, "applied"),
    ]);
    expect(screen.getByText("Stop refused: The request is already stopping.")).toBeVisible();
    expect(screen.queryByText("Pause: applied")).toBeNull();
  });
});

describe("bulk defer", () => {
  const cards = [
    card({ id: 1, issue: 101, column: "NeedsRob", question: EFFECT_FAILED, title: "Fix the build" }),
    card({ id: 3, issue: 103, column: "NeedsRob", question: { ...EFFECT_FAILED, id: 12 }, title: "Tidy docs" }),
  ];

  it("names how many shown questions it covers when the board is filtered", () => {
    setup(<DeferAll cards={cards} filtered />);
    expect(screen.getByRole("button", { name: "Let TL3 decide on all shown (2)" })).toBeVisible();
  });

  it("shows TL3's refusal on a question that was answered meanwhile", async () => {
    const user = userEvent.setup();
    const { filed, rerender } = setup(
      <>
        <DeferAll cards={cards} />
        {cards.map((shown) => (
          <CardCommands key={shown.id} card={shown} />
        ))}
      </>,
    );
    await user.click(screen.getByRole("button", { name: "Let TL3 decide on all" }));
    await user.click(screen.getByRole("button", { name: "Apply 2 choices" }));
    const [first, second] = filed;
    act(() =>
      rerender([
        record(second!.id, 3, second!.action, "refused", "Question 12 was answered meanwhile."),
        record(first!.id, 1, first!.action, "applied"),
      ]),
    );
    expect(screen.getByText("Let TL3 decide on question 9: applied")).toBeVisible();
    expect(
      screen.getByText("Let TL3 decide on question 12 refused: Question 12 was answered meanwhile."),
    ).toBeVisible();
  });
});
