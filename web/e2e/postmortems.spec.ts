import { expect, test } from "@playwright/test";
import { startFixture } from "./fixture";

test("postmortem command settles on the stream and the list opens the drawer", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.goto(fixture.url);
    await page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).click();
    const drawer = page.getByRole("dialog");
    await drawer.getByRole("tab", { name: "Postmortem" }).click();
    await expect(drawer.getByText("Judge's findings arrive when the postmortem runs")).toBeVisible();
    // This request already owes one for its escalation.
    await expect(drawer.getByRole("button", { name: "Write postmortem" })).toBeDisabled();
    await page.keyboard.press("Escape");
    await page.getByRole("link", { name: "Postmortems", exact: true }).click();
    await page.getByRole("searchbox", { name: "Search postmortems" }).fill("calibration");
    await page.getByRole("button", { name: "Search", exact: true }).click();
    await page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).first().click();
    await expect(drawer).toBeVisible();
    await expect(drawer.getByRole("tab", { name: "Postmortem" })).toHaveAttribute("aria-selected", "true");
  } finally { await fixture.stop(); }
});

test("postmortem screenshots in light and dark", async ({ page }) => {
  test.skip(!process.env.TL3_SCREENSHOTS, "set TL3_SCREENSHOTS to record evidence");
  const fixture = await startFixture();
  try {
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto(`${fixture.url}#/postmortems`);
      await expect(page.getByRole("heading", { name: "Postmortems", exact: true })).toBeVisible();
      await expect(page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).first()).toBeVisible();
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/postmortems-${scheme}.png` });
      await page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).first().click();
      await expect(page.getByText("Judge's findings arrive when the postmortem runs")).toBeVisible();
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/postmortem-drawer-${scheme}.png` });
    }
  } finally { await fixture.stop(); }
});
