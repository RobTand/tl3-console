import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { PromptVersion, RolePrompt } from "../../api/types.gen";
import { PromptsPage } from "./PromptsPage";

// CodeMirror needs layout jsdom lacks. Playwright drives the real editor and diff.
vi.mock("./CodeViews", () => ({
  PromptEditor: (props: { initial: string; label: string; onChange: (text: string) => void }) => (
    <textarea aria-label={props.label} defaultValue={props.initial} onChange={(e) => props.onChange(e.target.value)} />
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

/** A console holding one role's prompt, answering like crates/console/src/prompts.rs. */
function serve(initial: RolePrompt) {
  let prompt = initial;
  const requests: Array<{ method: string; url: string; headers: Record<string, string>; body: unknown }> = [];
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit = {}) => {
      const method = init.method ?? "GET";
      const body = init.body ? JSON.parse(init.body as string) : undefined;
      requests.push({ method, url, headers: init.headers as Record<string, string>, body });
      if (url === "/api/prompts") {
        const active = prompt.versions.find((v) => v.id === prompt.active)!;
        return json([
          {
            role: prompt.role,
            active_number: active.number,
            active_builtin: active.builtin,
            builtin_update: prompt.builtin_update !== null,
          },
        ]);
      }
      if (url === "/api/prompts/reviewer") return json(prompt);
      if (url === "/api/prompts/reviewer/versions") {
        if (body.base !== prompt.active) return new Response("Another version became active.\n", { status: 409 });
        const id = Math.max(...prompt.versions.map((v) => v.id)) + 1;
        const saved = version(id, prompt.versions[0]!.number + 1, { text: body.text, note: body.note, created_ms: NOW });
        prompt = { ...prompt, active: id, activated_ms: NOW, versions: [saved, ...prompt.versions] };
        return json(prompt, 201);
      }
      const activate = url.match(/^\/api\/prompts\/reviewer\/activate\/(\d+)$/);
      if (activate) {
        prompt = { ...prompt, active: Number(activate[1]) };
        if (prompt.active === prompt.builtin_update) prompt = { ...prompt, builtin_update: null };
        return json(prompt);
      }
      if (url.startsWith("/api/prompts/reviewer/keep/")) {
        prompt = { ...prompt, builtin_update: null };
        return json(prompt);
      }
      return new Response("not found", { status: 404 });
    }),
  );
  return requests;
}

function show(prompt: RolePrompt) {
  const requests = serve(prompt);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PromptsPage role="reviewer" />
    </QueryClientProvider>,
  );
  return requests;
}

const EDITED: RolePrompt = {
  role: "reviewer",
  active: 2,
  activated_by: "Rob",
  activated_ms: NOW - 60_000,
  builtin_update: null,
  versions: [version(2, 2), version(1, 1, { builtin: true, author: "TL3 deploy", note: "Shipped with TL3." })],
};

afterEach(() => vi.unstubAllGlobals());

describe("PromptsPage", () => {
  it("lists roles and shows the active version and its history with author, time and note", async () => {
    show(EDITED);
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
    const requests = show(EDITED);
    const save = await screen.findByRole("button", { name: "Save as v3" });
    expect(save).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Reviewer prompt, Markdown" }), " Check the tests.");
    expect(save).toBeDisabled();
    await user.click(screen.getByRole("radio", { name: "Changes +1 −1" }));
    expect(screen.getByLabelText("Changes from v2")).toHaveTextContent("Review prompt v2. Check the tests.");
    await user.type(screen.getByRole("textbox", { name: "Note" }), "Ask for the tests first.");
    await user.click(save);
    expect(await screen.findByText(/v3 active · edited/)).toBeVisible();
    const post = requests.find((r) => r.method === "POST")!;
    expect(post.url).toBe("/api/prompts/reviewer/versions");
    expect(post.headers["X-TL3-Console"]).toBe("1");
    expect(post.body).toEqual({ text: "Review prompt v2. Check the tests.", note: "Ask for the tests first.", base: 2 });
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Reviewer prompt, Markdown" })).toHaveValue(
      "Review prompt v2. Check the tests.",
    );
  });

  it("reverts to an older version in one click", async () => {
    const user = userEvent.setup();
    const requests = show(EDITED);
    await user.click(await screen.findByRole("button", { name: "Revert to v1" }));
    expect(await screen.findByText(/v1 active · built-in/)).toBeVisible();
    expect(requests.filter((r) => r.method === "POST").map((r) => r.url)).toEqual(["/api/prompts/reviewer/activate/1"]);
  });

  it("won't revert over an unsaved edit", async () => {
    const user = userEvent.setup();
    show(EDITED);
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " More.");
    expect(screen.getByRole("button", { name: "Revert to v1" })).toBeDisabled();
  });

  it("shows a refused save's reason", async () => {
    const user = userEvent.setup();
    serve(EDITED);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <PromptsPage role="reviewer" />
      </QueryClientProvider>,
    );
    await user.type(await screen.findByRole("textbox", { name: "Reviewer prompt, Markdown" }), " More.");
    await user.type(screen.getByRole("textbox", { name: "Note" }), "Why.");
    // Another tab activated v1 after this one loaded.
    await (globalThis.fetch as (url: string, init: RequestInit) => Promise<Response>)(
      "/api/prompts/reviewer/activate/1",
      { method: "POST" },
    );
    await user.click(screen.getByRole("button", { name: "Save as v3" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Another version became active.");
  });

  it("flags a built-in update with its diff, and keeps Rob's version on request", async () => {
    const user = userEvent.setup();
    const shipped = version(3, 3, { builtin: true, author: "TL3 deploy", text: "Review prompt, new release." });
    const requests = show({ ...EDITED, builtin_update: 3, versions: [shipped, ...EDITED.versions] });
    const banner = await screen.findByRole("region", { name: "Built-in update" });
    expect(banner).toHaveTextContent("A deploy shipped built-in v3 while v2 is active.");
    await user.click(within(banner).getByRole("button", { name: "Compare" }));
    expect(screen.getByLabelText("Changes from v2 to v3")).toHaveTextContent("Review prompt, new release.");
    await user.click(within(banner).getByRole("button", { name: "Keep v2" }));
    await expect.poll(() => screen.queryByRole("region", { name: "Built-in update" })).toBeNull();
    expect(requests.some((r) => r.method === "POST" && r.url === "/api/prompts/reviewer/keep/3")).toBe(true);
  });
});
