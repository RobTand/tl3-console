import { expect, test } from "@playwright/test";
import type { RolePrompt } from "../src/api/types.gen";
import { startFixture } from "./fixture";

const LINE = "Keep every note under three sentences.";

test("Rob edits a prompt, sees the diff, saves it as the active version, and reverts it", async ({ page }) => {
  const fixture = await startFixture();
  const read = async () => (await (await page.request.get(`${fixture.url}/api/prompts/writer`)).json()) as RolePrompt;
  try {
    await page.goto(`${fixture.url}/#/prompts/writer`);
    await expect(page.getByText(/v1 active · built-in/)).toBeVisible();
    const editor = page.getByRole("textbox", { name: "Writer prompt, Markdown" });
    await editor.click();
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.type(`\n${LINE}`);
    await page.getByRole("radio", { name: /^Changes \+/ }).click();
    const diff = page.getByLabel("Changes from v1");
    await expect(diff.locator(".cm-changedLine", { hasText: LINE })).toBeVisible();
    // An added line shows as added, with no removed blank line beside it.
    await expect(diff.locator(".cm-deletedChunk")).toHaveCount(0);

    await page.getByRole("textbox", { name: "Note" }).fill("Shorter notes for Rob.");
    await page.getByRole("button", { name: "Save as v2" }).click();
    await expect(page.getByText(/v2 active · edited · set by Rob/)).toBeVisible();
    const history = page.getByRole("complementary", { name: "Version history" });
    await expect(history.getByRole("listitem", { name: "Version 2" })).toContainText("Active");
    await expect(history.getByRole("listitem", { name: "Version 2" })).toContainText("Shorter notes for Rob.");
    const saved = await read();
    expect(saved.active.id).toBe(saved.versions[0]!.id);
    expect(saved.active.text).toContain(LINE);

    await history.getByRole("button", { name: "Revert to v1" }).click();
    await expect(page.getByText(/v1 active · built-in/)).toBeVisible();
    await expect(editor).not.toContainText(LINE);
    const reverted = await read();
    expect(reverted.active.id).toBe(reverted.versions[1]!.id);
    expect(reverted.versions).toHaveLength(2);
  } finally {
    await fixture.stop();
  }
});

test("Rob keeps his version over a built-in update", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.goto(`${fixture.url}/#/prompts/reviewer`);
    const banner = page.getByRole("region", { name: "Built-in update" });
    await expect(banner).toContainText("A deploy shipped built-in v3 while v2 is active.");
    await banner.getByRole("button", { name: "Compare built-in v3" }).click();
    await expect(page.getByLabel("Changes from v2 to v3")).toContainText("Name the file and line of every finding.");
    await banner.getByRole("button", { name: "Keep v2" }).click();
    await expect(banner).toHaveCount(0);
    await expect(page.getByRole("navigation", { name: "Roles" }).getByText("Update")).toHaveCount(0);
    await page.reload();
    await expect(page.getByText(/v2 active · edited/)).toBeVisible();
    await expect(page.getByRole("region", { name: "Built-in update" })).toHaveCount(0);
  } finally {
    await fixture.stop();
  }
});

test("a save that loses to another tab keeps Rob's edit and diffs it against the winner", async ({ page }) => {
  const fixture = await startFixture();
  const api = `${fixture.url}/api/prompts/planner`;
  try {
    await page.goto(`${fixture.url}/#/prompts/planner`);
    await expect(page.getByText(/v1 active · built-in/)).toBeVisible();
    await page.getByRole("textbox", { name: "Planner prompt, Markdown" }).click();
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.type(`
${LINE}`);
    await page.getByRole("textbox", { name: "Note" }).fill("Mine.");
    // Another tab saves first.
    const before = (await (await page.request.get(api)).json()) as RolePrompt;
    const other = await page.request.post(`${api}/versions`, {
      headers: { "X-TL3-Console": "1" },
      data: { text: `${before.active.text}
From the other tab.
`, note: "Other tab.", base: before.active.id },
    });
    expect(other.status()).toBe(201);
    await page.getByRole("button", { name: "Save as v2" }).click();
    await expect(page.getByRole("status").filter({ hasText: "became active while you edited" })).toContainText(
      "v2 became active while you edited. Your edit is kept",
    );
    const diff = page.getByLabel("Changes from v2");
    await expect(diff.locator(".cm-changedLine", { hasText: LINE })).toBeVisible();
    await expect(diff.locator(".cm-deletedChunk", { hasText: "From the other tab." })).toBeVisible();
    await expect(page.getByRole("textbox", { name: "Note" })).toHaveValue("Mine.");
    await page.getByRole("button", { name: "Save as v3" }).click();
    await expect(page.getByText(/v3 active · edited/)).toBeVisible();
    const after = (await (await page.request.get(api)).json()) as RolePrompt;
    expect(after.active.text).toContain(LINE);
    expect(after.versions.map((v) => v.note)).toEqual(["Mine.", "Other tab.", "The built-in prompt TL3 shipped with."]);
  } finally {
    await fixture.stop();
  }
});

test("prompt page screenshots in both themes", async ({ page }) => {
  test.skip(!process.env.TL3_SCREENSHOTS, "set TL3_SCREENSHOTS to a directory to record them");
  const fixture = await startFixture();
  try {
    await page.setViewportSize({ width: 1600, height: 1000 });
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto(`${fixture.url}/#/prompts/reviewer`);
      // Start each theme from a fresh page, without the last theme's edit.
      await page.reload();
      await expect(page.getByRole("region", { name: "Built-in update" })).toBeVisible();
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/prompts-${scheme}.png` });
      await page.getByRole("textbox", { name: "Reviewer prompt, Markdown" }).click();
      await page.keyboard.press("ControlOrMeta+End");
      await page.keyboard.type(`\n${LINE}`);
      await page.getByRole("radio", { name: /^Changes/ }).click();
      await page.getByRole("textbox", { name: "Note" }).fill("Shorter review notes.");
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/prompts-diff-${scheme}.png` });
    }
  } finally {
    await fixture.stop();
  }
});
