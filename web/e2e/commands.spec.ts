import { expect, test } from "@playwright/test";
import { startFixture } from "./fixture";

test("pause from the card: pending, then applied, then the badge", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.goto(fixture.url);
    await expect(page.getByRole("status")).toHaveText(/Live/);
    const card = page.getByRole("article", { name: /tl3-sandbox#42:/ });
    await expect(card.getByRole("listitem").filter({ hasText: "Paused" })).toHaveCount(0);
    await card.getByRole("button", { name: "Pause" }).click();
    await card.getByRole("button", { name: "Confirm pause" }).click();
    // The fixture's controller applies commands only when told, so pending is visible.
    await expect(card.getByText("Pause: pending")).toBeVisible();
    await fixture.apply();
    await expect(card.getByText("Pause: applied")).toBeVisible();
    await expect(card.getByRole("listitem").filter({ hasText: "Paused" })).toBeVisible();
    await expect(card.getByRole("button", { name: "Resume" })).toBeVisible();
  } finally {
    await fixture.stop();
  }
});

test("a Needs Rob card shows its options, and Let TL3 decide moves it on", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.goto(fixture.url);
    const needsRob = page.getByRole("region", { name: "Needs Rob" });
    const card = needsRob.getByRole("article", { name: /tl3-sandbox#43:/ });
    await expect(card).toBeVisible();
    const question = card.getByRole("region", { name: /^Question \d+$/ });
    await expect(question.getByText("A required action needs your help before work can continue.")).toBeVisible();
    const options = question.getByRole("list", { name: "Options" }).getByRole("button");
    await expect(options).toHaveText(["Continue after fixing the causeRecommended", "Stop"]);
    await expect(question.getByText(/TL3 recommends “Continue after fixing the cause”/)).toBeVisible();
    await question.getByRole("button", { name: "Let TL3 decide" }).click();
    await expect(card.getByText(/Let TL3 decide on question \d+: pending/)).toBeVisible();
    await fixture.apply();
    await expect(needsRob.getByRole("article", { name: /tl3-sandbox#43:/ })).toHaveCount(0);
    await expect(page.getByRole("article", { name: /tl3-sandbox#43:/ })).toBeVisible();
  } finally {
    await fixture.stop();
  }
});
