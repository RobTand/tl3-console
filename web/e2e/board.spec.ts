import { expect, test } from "@playwright/test";
import { startFixture } from "./fixture";

const LABELS: Record<string, string> = { NeedsRob: "Needs Rob" };

test("a controller commit moves a card on the open board", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.goto(fixture.url);
    const card = (region: string) =>
      page.getByRole("region", { name: region }).getByRole("article", { name: /tl3-sandbox#42:/ });
    await expect(card("Intake")).toBeVisible();
    // The example config's request reader is default-model; cards name it briefly.
    await expect(card("Intake").getByText("Reader · default-model", { exact: true })).toBeVisible();
    await expect(page.getByRole("status")).toHaveText(/Live/);
    // A reload would drop this marker.
    await page.evaluate(() => ((window as unknown as { tl3Marker: number }).tl3Marker = 1));
    const column = await fixture.advance();
    expect(column).not.toBe("Intake");
    await expect(card(LABELS[column] ?? column)).toBeVisible();
    await expect(card("Intake")).toHaveCount(0);
    expect(await page.evaluate(() => (window as unknown as { tl3Marker?: number }).tl3Marker)).toBe(1);
  } finally {
    await fixture.stop();
  }
});

test("board screenshots in both themes", async ({ page }) => {
  test.skip(!process.env.TL3_SCREENSHOTS, "set TL3_SCREENSHOTS to a directory to record them");
  const fixture = await startFixture();
  try {
    // Wide enough to show every column.
    await page.setViewportSize({ width: 2720, height: 1100 });
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto(fixture.url);
      await expect(page.getByRole("status")).toHaveText(/Live/);
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/board-${scheme}.png` });
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/board-phone.png` });
    // A card with its issue link at rest and under the pointer.
    await page.setViewportSize({ width: 1600, height: 1000 });
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto(fixture.url);
      const card = page.getByRole("region", { name: "Needs Rob" }).getByRole("article", { name: /tl3-sandbox#43:/ });
      await expect(card).toBeVisible();
      await card.getByRole("link", { name: "tl3-sandbox#43" }).hover();
      // A clip of the page keeps the pointer where it is; an element shot would not.
      const box = (await card.boundingBox())!;
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/issue-link-hover-${scheme}.png`, clip: box });
      await card.getByRole("link", { name: "tl3-sandbox#43" }).focus();
      await page.keyboard.press("Shift+Tab");
      await page.keyboard.press("Tab");
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/issue-link-focus-${scheme}.png`, clip: box });
    }
  } finally {
    await fixture.stop();
  }
});

test("an issue reference opens GitHub in a new tab and leaves the case file closed", async ({ page, context }) => {
  const fixture = await startFixture();
  try {
    // The tab only has to be asked for; the test never needs GitHub itself.
    await context.route("https://github.com/**", (route) =>
      route.fulfill({ status: 200, contentType: "text/html", body: "<title>GitHub</title>" }),
    );
    await page.goto(fixture.url);
    const card = page.getByRole("region", { name: "Needs Rob" }).getByRole("article", { name: /tl3-sandbox#43:/ });
    const link = card.getByRole("link", { name: "tl3-sandbox#43" });
    const issue = "https://github.com/RobTand/tl3-sandbox/issues/43";
    await expect(link).toHaveAttribute("href", issue);
    await expect(link).toHaveAttribute("target", "_blank");
    // At rest it reads as the card's muted text; under the pointer it underlines.
    const name = link.locator("span").first();
    await expect(name).toHaveCSS("text-decoration-line", "none");
    await link.hover();
    await expect(name).toHaveCSS("text-decoration-line", "underline");

    const clicked = context.waitForEvent("page");
    await link.click();
    const tab = await clicked;
    await tab.waitForURL(issue);
    await tab.close();
    expect(new URL(page.url()).search).toBe("");
    await expect(page.getByRole("dialog")).toHaveCount(0);

    // The same from the keyboard: the card, then its issue link.
    await card.focus();
    await page.keyboard.press("Tab");
    await expect(link).toBeFocused();
    const entered = context.waitForEvent("page");
    await page.keyboard.press("Enter");
    const second = await entered;
    await second.waitForURL(issue);
    await second.close();
    expect(new URL(page.url()).search).toBe("");
    await expect(page.getByRole("dialog")).toHaveCount(0);

    // The drawer's header links to the same page as the case file's own link.
    await card.getByRole("heading").getByRole("link").click();
    const drawer = page.getByRole("dialog");
    await expect(drawer).toBeVisible();
    const header = drawer.getByRole("link", { name: "RobTand/tl3-sandbox#43", exact: true });
    await expect(header).toHaveAttribute("href", issue);
    await expect(drawer.getByRole("link", { name: "Issue RobTand/tl3-sandbox#43 on GitHub" })).toHaveAttribute("href", issue);
  } finally {
    await fixture.stop();
  }
});
