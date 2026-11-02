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
    await page.getByRole("radio", { name: /^Changes \+1/ }).click();
    await expect(page.getByLabel("Changes from v1").locator(".cm-changedLine", { hasText: LINE })).toBeVisible();

    await page.getByRole("textbox", { name: "Note" }).fill("Shorter notes for Rob.");
    await page.getByRole("button", { name: "Save as v2" }).click();
    await expect(page.getByText(/v2 active · edited · set by Rob/)).toBeVisible();
    const history = page.getByRole("complementary", { name: "Version history" });
    await expect(history.getByRole("listitem", { name: "Version 2" })).toContainText("Active");
    await expect(history.getByRole("listitem", { name: "Version 2" })).toContainText("Shorter notes for Rob.");
    const saved = await read();
    expect(saved.active).toBe(saved.versions[0]!.id);
    expect(saved.versions[0]!.text).toContain(LINE);

    await history.getByRole("button", { name: "Revert to v1" }).click();
    await expect(page.getByText(/v1 active · built-in/)).toBeVisible();
    await expect(editor).not.toContainText(LINE);
    const reverted = await read();
    expect(reverted.active).toBe(reverted.versions[1]!.id);
    expect(reverted.versions).toHaveLength(2);
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
