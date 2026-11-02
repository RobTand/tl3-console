import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { PromptSummary, PromptVersion, RolePrompt } from "../../api/types.gen";
import { href, useRoute } from "../../route";
import { PromptsPage } from "./PromptsPage";

// CodeMirror needs layout jsdom lacks. Playwright drives the real editor and diff.
vi.mock("./CodeViews", () => ({
  PromptEditor: (props: { initial: string; label: string; onChange: (text: string) => void; autoFocus?: boolean }) => (
    <textarea
      aria-label={props.label}
      defaultValue={props.initial}
      autoFocus={props.autoFocus}
      onChange={(e) => props.onChange(e.target.value)}
    />
  ),
  PromptDiff: (props: { before: string; after: string; label: string }) => (
    <pre aria-label={props.label}>{`${props.before}\n=>\n${props.after}`}</pre>
  ),
}));

const NOW = Date.now();

function version(id: number, number: number, fields: Partial<PromptVersion> = {}): PromptVersion {
  return {
    id,
    number,
    text: `Review prompt v${number}.`,
    builtin: false,
    author: "Rob",
    note: `Note for v${number}.`,
    created_ms: NOW - 3 * 60_000,
    ...fields,
  };
}

function summary({ text: _text, ...rest }: PromptVersion): PromptSummary {
  return rest;
}

/** A console holding one role's versions, answering like crates/console/src/prompts.rs. */
function serve(initial: PromptVersion[], active: number, update: number | null = null) {
  const versions = [...initial];
  const state = { active, update };
  const requests: Array<{ method: string; url: string; headers: Record<string, string>; body: unknown }> = [];
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
  const find = (id: number) => versions.find((v) => v.id === id)!;
  const role = (): RolePrompt => ({
    role: "reviewer",
    active: find(state.active),
    activated_by: "Rob",
    activated_ms: NOW - 60_000,
    builtin_update: state.update === null ? null : find(state.update),
    versions: versions.map(summary),
  });
  const saveVersion = (text: string, note: string) => {
    const id = Math.max(...versions.map((v) => v.id)) + 1;
    versions.unshift(version(id, versions[0]!.number + 1, { text, note, created_ms: NOW }));
    state.active = id;
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit = {}) => {
      const method = init.method ?? "GET";
      const body = init.body ? JSON.parse(init.body as string) : undefined;
      requests.push({ method, url, headers: init.headers as Record<string, string>, body });
      if (url === "/api/prompts") {
        const current = find(state.active);
        return json([
          {
            role: "reviewer",
            active_number: current.number,
            active_builtin: current.builtin,
            builtin_update: state.update !== null,
          },
          { role: "writer", active_number: 1, active_builtin: true, builtin_update: false },
        ]);
      }
      if (url === "/api/prompts/reviewer") return json(role());
      if (url === "/api/prompts/writer") return json({ ...role(), role: "writer" });
      const one = url.match(/^\/api\/prompts\/reviewer\/versions\/(\d+)$/);
      if (one) return json(find(Number(one[1])));
      if (url === "/api/prompts/reviewer/versions") {
        if (body.base !== state.active) return new Response("Another version became active while you edited.\n", { status: 409 });
        saveVersion(body.text, body.note);
        return json(role(), 201);
      }
      const activate = url.match(/^\/api\/prompts\/reviewer\/activate\/(\d+)$/);
      if (activate) {
        if (body.expected !== state.active) {
          return new Response("Another version became active after this page loaded. Check the history, then choose again.\n", {
            status: 409,
          });
        }
        state.active = Number(activate[1]);
        if (state.active === state.update) state.update = null;
        return json(role());
      }
      if (url.startsWith("/api/prompts/reviewer/keep/")) {
        if (state.update === null) return new Response("No built-in update waits for that version.", { status: 409 });
        state.update = null;
        return json(role());
      }
      return new Response("not found", { status: 404 });
    }),
  );
  return {
    requests,
    keepElsewhere: () => { state.update = null; },
    /** Another tab saves a version. */
    saveElsewhere: (text: string) => saveVersion(text, "From another tab."),
  };
}

function show(versions: PromptVersion[], active: number, update: number | null = null) {
  const server = serve(versions, active, update);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PromptsPage role="reviewer" />
    </QueryClientProvider>,
  );
  return { ...server, client };
}

const HISTORY = [version(2, 2), version(1, 1, { builtin: true, author: "TL3 deploy", note: "Shipped with TL3." })];
const posts = (requests: Array<{ method: string; url: string }>) =>
  requests.filter((r) => r.method === "POST").map((r) => r.url);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

describe("PromptsPage", () => {
  it("lists roles and shows the active version and its history with author, time and note", async () => {
    show(HISTORY, 2);
    const roles = await screen.findByRole("navigation", { name: "Roles" });
    expect(await within(roles).findByRole("link", { name: /Reviewer/ })).toHaveAttribute("aria-current", "page");
    expect(within(roles).getByText("v2 · Edited")).toBeVisible();
    expect(await screen.findByText(/v2 active · edited · set by Rob/)).toBeVisible();
    const history = screen.getByRole("complementary", { name: "Version history" });
    const first = within(history).getByRole("listitem", { name: "Version 1" });
    expect(within(first).getByText("TL3 deploy")).toBeVisible();
    expect(within(first).getByText("Shipped with TL3.")).toBeVisible();
    expect(within(first).getByText("Built-in")).toBeVisible();
    expect(within(first).getByText("3m ago")).toBeVisible();
    expect(within(history).getByRole("listitem", { name: "Version 2" })).toHaveTextContent("Active");
  });

  it("saves an edit with a required note against the active version, then shows it active", async () => {
    const user = userEvent.setup();
    const { requests } = show(HISTORY, 2);
    const save = await screen.findByRole("button", { name: "Save as v3" });
    expect(save).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Reviewer prompt, Markdown" }), " Check the tests.");
    expect(save).toBeDisabled();
    await user.click(screen.getByRole("radio", { name: "Changes +1 −1" }));
    expect(screen.getByLabelText("Changes from v2")).toHaveTextContent("Review prompt v2. Check the tests.");
    await user.type(screen.getByRole("textbox", { name: "Note" }), "Ask for the tests first.");
    await user.click(screen.getByRole("button", { name: "Save as v3" }));
    expect(await screen.findByText(/v3 active · edited/)).toBeVisible();
    const post = requests.find((r) => r.method === "POST")!;
    expect(post.url).toBe("/api/prompts/reviewer/versions");
    expect(post.headers["X-TL3-Console"]).toBe("1");
    expect(post.body).toEqual({ text: "Review prompt v2. Check the tests.", note: "Ask for the tests first.", base: 2 });
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveValue("");
    const editor = screen.getByRole("textbox", { name: "Reviewer prompt, Markdown" });
    expect(editor).toHaveValue("Review prompt v2. Check the tests.");
    expect(editor).toHaveFocus();
  });

  it("keeps the draft and note after a refetch changes the active version, and saves against the new base", async () => {
    const user = userEvent.setup();
    const { saveElsewhere, client, requests } = show(HISTORY, 2);
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " Mine.");
    await user.type(screen.getByRole("textbox", { name: "Note" }), "Why.");
    saveElsewhere("Review prompt from another tab.");
    await act(() => client.invalidateQueries({ queryKey: ["prompts", "reviewer"] }));
    expect(await screen.findByText(/v3 active/)).toBeVisible();
    expect(screen.getByRole("textbox", { name: "Reviewer prompt, Markdown" })).toHaveValue("Review prompt v2. Mine.");
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveValue("Why.");
    expect(screen.getByRole("status")).toHaveTextContent("Your edit is kept; check its changes against v3, then save.");
    expect(screen.getByLabelText("Changes from v3")).toHaveTextContent("Review prompt from another tab. => Review prompt v2. Mine.");
    await user.click(screen.getByRole("button", { name: "Save as v4" }));
    expect(await screen.findByText(/v4 active/)).toBeVisible();
    expect(requests.filter((r) => r.method === "POST").map((r) => r.body)).toEqual([
      { text: "Review prompt v2. Mine.", note: "Why.", base: 3 },
    ]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it.each(["role", "Board", "back"])("asks before discarding unsaved text on %s navigation", async (target) => {
    window.history.replaceState(null, "", "#/prompts/reviewer");
    const user = userEvent.setup();
    serve(HISTORY, 2);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    function Page() {
      const route = useRoute();
      return <>
        <a href={href({ page: "board" })}>Board</a>
        {route.page === "prompts" ? <PromptsPage role={route.role} /> : <p>The board</p>}
      </>;
    }
    render(<QueryClientProvider client={client}><Page /></QueryClientProvider>);
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " Mine.");
    const leave = async () => {
      if (target === "back") {
        await act(() => {
          window.history.replaceState(null, "", "#/");
          window.dispatchEvent(new HashChangeEvent("hashchange"));
        });
      } else {
        await user.click(screen.getByRole("link", { name: target === "role" ? /Writer/ : "Board" }));
      }
    };
    await leave();
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("textbox", { name: "Reviewer prompt, Markdown" })).toHaveValue("Review prompt v2. Mine.");
    expect(window.location.hash).toBe("#/prompts/reviewer");
    confirm.mockReturnValue(true);
    await leave();
    expect(confirm).toHaveBeenCalledTimes(2);
    if (target === "role") {
      expect(await screen.findByRole("textbox", { name: "Writer prompt, Markdown" })).toHaveValue("Review prompt v2.");
    } else {
      expect(await screen.findByText("The board")).toBeVisible();
    }
  });

  it("asks before closing the page with unsaved text, and stops guarding after Discard", async () => {
    const user = userEvent.setup();
    show(HISTORY, 2);
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " Mine.");
    const closing = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(closing);
    expect(closing.defaultPrevented).toBe(true);
    await user.click(screen.getByRole("button", { name: "Discard" }));
    const clean = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(clean);
    expect(clean.defaultPrevented).toBe(false);
  });

  it("keeps Rob's edit when a save loses to another tab, and diffs it against the new version", async () => {
    const user = userEvent.setup();
    const { saveElsewhere, requests } = show(HISTORY, 2);
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " Mine.");
    await user.type(screen.getByRole("textbox", { name: "Note" }), "Why.");
    saveElsewhere("Review prompt from another tab.");
    await user.click(screen.getByRole("button", { name: "Save as v3" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "v3 became active while you edited. Your edit is kept; check its changes against v3, then save.",
    );
    expect(screen.getByLabelText("Changes from v3")).toHaveTextContent(
      "Review prompt from another tab. => Review prompt v2. Mine.",
    );
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveValue("Why.");
    expect(screen.queryByRole("alert")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Save as v4" }));
    expect(await screen.findByText(/v4 active/)).toBeVisible();
    const saves = requests.filter((r) => r.method === "POST");
    expect(saves.map((r) => (r.body as { base: number }).base)).toEqual([2, 3]);
  });

  it("reverts to an older version in one click, naming the version it saw active", async () => {
    const user = userEvent.setup();
    const { requests } = show(HISTORY, 2);
    await user.click(await screen.findByRole("button", { name: "Revert to v1" }));
    expect(await screen.findByText(/v1 active · built-in/)).toBeVisible();
    expect(posts(requests)).toEqual(["/api/prompts/reviewer/activate/1"]);
    expect(requests.find((r) => r.method === "POST")!.body).toEqual({ expected: 2 });
  });

  it.each([
    { action: "Revert", active: 2, button: "Revert to v1", retry: "Revert to v1" },
    { action: "Activate", active: 1, button: "Activate v2", retry: "Revert to v2" },
  ])("refetches after a 409 from $action so retry uses the new active version", async ({ active, button, retry }) => {
    const user = userEvent.setup();
    const { saveElsewhere, requests } = show(HISTORY, active);
    const choose = await screen.findByRole("button", { name: button });
    saveElsewhere("Review prompt from another tab.");
    await user.click(choose);
    expect(await screen.findByText(/v3 active/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: retry }));
    expect(await screen.findByText(new RegExp(`v${active === 2 ? 1 : 2} active`))).toBeVisible();
    expect(requests.filter((r) => r.method === "POST").map((r) => r.body)).toEqual([
      { expected: active }, { expected: 3 },
    ]);
  });

  it("refetches after a 409 from Keep so the resolved update disappears", async () => {
    const user = userEvent.setup();
    const { keepElsewhere, requests } = show([version(3, 3, { builtin: true }), ...HISTORY], 2, 3);
    const keep = await screen.findByRole("button", { name: "Keep v2" });
    keepElsewhere();
    await user.click(keep);
    await expect.poll(() => screen.queryByRole("region", { name: "Built-in update" })).toBeNull();
    expect(requests.filter((r) => r.url === "/api/prompts/reviewer" && r.method === "GET")).toHaveLength(2);
  });

  it("won't revert over an unsaved edit, and says why in visible text", async () => {
    const user = userEvent.setup();
    show(HISTORY, 2);
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " More.");
    const revert = screen.getByRole("button", { name: "Revert to v1" });
    expect(revert).toBeDisabled();
    expect(revert).toHaveAccessibleDescription(
      "Save or discard your edit before reverting or choosing another version.",
    );
    expect(revert).not.toHaveAttribute("title");
  });

  it("puts the keyboard back in the editor after Discard", async () => {
    const user = userEvent.setup();
    show(HISTORY, 2);
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " More.");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    const editor = screen.getByRole("textbox", { name: "Reviewer prompt, Markdown" });
    expect(editor).toHaveValue("Review prompt v2.");
    expect(editor).toHaveFocus();
  });

  it("fetches a version's text only to compare it, and returns focus on Close", async () => {
    const user = userEvent.setup();
    const { requests } = show(HISTORY, 2);
    const compare = await screen.findByRole("button", { name: "Compare v1" });
    expect(requests.some((r) => r.url.includes("/versions/"))).toBe(false);
    await user.click(compare);
    expect(await screen.findByLabelText("Changes from v2 to v1")).toHaveTextContent("Review prompt v1.");
    expect(requests.filter((r) => r.url === "/api/prompts/reviewer/versions/1")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.getByRole("button", { name: "Compare v1" })).toHaveFocus();
  });

  it("flags a built-in update with its diff, and keeps Rob's version on request", async () => {
    const user = userEvent.setup();
    const shipped = version(3, 3, { builtin: true, author: "TL3 deploy", text: "Review prompt, new release." });
    const { requests } = show([shipped, ...HISTORY], 2, 3);
    const banner = await screen.findByRole("region", { name: "Built-in update" });
    expect(banner).toHaveTextContent("A deploy shipped built-in v3 while v2 is active.");
    await user.click(within(banner).getByRole("button", { name: "Compare built-in v3" }));
    expect(await screen.findByLabelText("Changes from v2 to v3")).toHaveTextContent("Review prompt, new release.");
    await user.click(within(banner).getByRole("button", { name: "Keep v2" }));
    await expect.poll(() => screen.queryByRole("region", { name: "Built-in update" })).toBeNull();
    expect(posts(requests)).toContain("/api/prompts/reviewer/keep/3");
  });
});
