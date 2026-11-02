import { expect, test } from "@playwright/test";
import { startFixture } from "./fixture";

test("opening a card shows its case file, and a commit adds to its timeline live", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.goto(fixture.url);
    await page
      .getByRole("article", { name: /tl3-sandbox#42:/ })
      .getByRole("link", { name: "Add a live board to the operator console" })
      .click();
    const drawer = page.getByRole("dialog", { name: "Add a live board to the operator console" });
    await expect(drawer).toBeVisible();
    await expect(page).toHaveURL(/\?request=\d+$/);
    const timeline = drawer.getByRole("region", { name: "Timeline" }).getByRole("listitem");
    await expect(timeline.first()).toContainText("Rob labelled the issue");
    const before = await timeline.count();
    await fixture.advance();
    await expect(timeline).toHaveCount(before + 1);
    await page.keyboard.press("Escape");
    await expect(drawer).toHaveCount(0);
    await expect(page).not.toHaveURL(/request=/);
  } finally {
    await fixture.stop();
  }
});

test("case file screenshots in both themes", async ({ page }) => {
  test.skip(!process.env.TL3_SCREENSHOTS, "set TL3_SCREENSHOTS to a directory to record them");
  const fixture = await startFixture();
  try {
    await page.setViewportSize({ width: 1600, height: 1000 });
    for (const [request, name] of [
      ["fix-round", "Cache compiled kernels between campaign rows"],
      ["needs-rob", "Quantize attention layers to FP8 with calibration"],
    ] as const) {
      for (const scheme of ["light", "dark"] as const) {
        await page.emulateMedia({ colorScheme: scheme });
        await page.goto(fixture.url);
        await page.getByRole("link", { name }).click();
        await expect(page.getByRole("dialog").getByRole("region", { name: "Timeline" })).toBeVisible();
        await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/case-file-${request}-${scheme}.png` });
      }
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(fixture.url);
    await page.getByRole("link", { name: "Remove the stale celestia worker from the fleet file" }).click();
    await expect(page.getByRole("dialog").getByRole("region", { name: "Timeline" })).toBeVisible();
    await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/case-file-phone.png` });
  } finally {
    await fixture.stop();
  }
});

test("a long title clears the close button on a phone", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(fixture.url);
    await page.getByRole("link", { name: "Remove the stale celestia worker from the fleet file" }).click();
    const close = await page.getByRole("button", { name: "Close the case file" }).boundingBox();
    const title = await page.getByRole("dialog").getByRole("heading", { level: 2 }).boundingBox();
    expect(title!.x + title!.width).toBeLessThanOrEqual(close!.x);
  } finally {
    await fixture.stop();
  }
});
