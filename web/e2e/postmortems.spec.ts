import { expect, test } from "@playwright/test";
import { startFixture } from "./fixture";

test("postmortem command settles on the stream and the list opens the drawer", async ({ page }) => {
  const fixture = await startFixture();
  try {
    await page.goto(fixture.url);
    await page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).click();
    const drawer = page.getByRole("dialog");
    await expect(drawer.getByRole("tab", { name: "Postmortem" })).toBeVisible();
    await drawer.getByRole("tab", { name: "Postmortem" }).click();
    await expect(drawer.getByText("Judge's findings arrive when the postmortem runs")).toBeVisible();
    // This request already owes one for its escalation.
    await expect(drawer.getByRole("button", { name: "Write postmortem" })).toBeDisabled();
    await fixture.finishPostmortem();
    await expect(drawer.getByText("Calibration needs representative input samples.")).toBeVisible();
    await expect(drawer.getByRole("button", { name: "Write postmortem" })).toBeEnabled();
    await drawer.getByRole("button", { name: "Write postmortem" }).click();
    await drawer.getByRole("button", { name: "Confirm postmortem" }).click();
    await expect(drawer.getByText("Write postmortem: pending")).toBeVisible();
    await fixture.apply();
    await expect(drawer.getByText("Write postmortem: applied")).toBeVisible();
    await expect(drawer.getByRole("button", { name: "Write postmortem" })).toBeDisabled();
    // A second tab can race the button. TL3's refusal must survive unchanged.
    const node = new URL(page.url()).searchParams.get("request");
    const response = await page.request.post(`${fixture.url}/api/requests/${node}/commands`, {
      headers: { "x-tl3-console": "1" },
      data: { id: "33700000-0000-4000-8000-000000000001", action: { kind: "postmortem" } },
    });
    expect(response.status()).toBe(202);
    await fixture.apply();
    await expect(
      drawer.getByText("Write postmortem refused: A postmortem for this request is already owed or running."),
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await page.getByRole("link", { name: "Postmortems", exact: true }).click();
    await expect(page.getByText("Seen in 3 postmortems").first()).toBeVisible();
    await page.getByRole("searchbox", { name: "Search postmortems" }).fill("calibration");
    await page.getByRole("button", { name: "Search", exact: true }).click();
    await page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).first().click();
    await expect(drawer).toBeVisible();
    await expect(drawer.getByRole("tab", { name: "Postmortem" })).toHaveAttribute("aria-selected", "true");
  } finally {
    await fixture.stop();
  }
});

test("postmortem screenshots in light and dark", async ({ page }) => {
  test.skip(!process.env.TL3_SCREENSHOTS, "set TL3_SCREENSHOTS to record evidence");
  const fixture = await startFixture();
  try {
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto(`${fixture.url}#/postmortems`);
      await expect(page.getByRole("heading", { name: "Postmortems", exact: true })).toBeVisible();
      await expect(
        page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).first(),
      ).toBeVisible();
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/postmortems-${scheme}.png` });
      await page.getByRole("link", { name: "Quantize attention layers to FP8 with calibration" }).first().click();
      await expect(page.getByText("Judge's findings arrive when the postmortem runs")).toBeVisible();
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/postmortem-drawer-${scheme}.png` });
      await page.keyboard.press("Escape");
      await page.getByRole("link", { name: "Shard the GPU benchmark queue by device class" }).first().click();
      await expect(page.getByRole("region", { name: "What went wrong" })).toBeVisible();
      await page.screenshot({ path: `${process.env.TL3_SCREENSHOTS}/postmortem-findings-${scheme}.png` });
    }
  } finally {
    await fixture.stop();
  }
});
