import { expect, test } from "@playwright/test";

async function waitForStableFonts(page: import("@playwright/test").Page) {
  await page.evaluate(() => document.fonts.ready);
}

test("fresh browser fixture exposes setup v2 without a skip path", async ({ page }) => {
  await page.goto("/?view=setup");

  await expect(
    page.getByRole("heading", { name: "Speak. It listens. It sets the words down." }),
  ).toBeVisible();
  await expect(page.getByText("First run")).toBeVisible();
  await expect(page.getByRole("button", { name: /skip setup/i })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /begin/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /Atmospeak companion/i })).toHaveCount(0);
});

test.describe("canonical editorial hub", () => {
  test.use({ viewport: { width: 1000, height: 660 } });

  test("Home matches the checked-in visual baseline", async ({ page }) => {
    await page.goto("/?view=hub&fixture=hub");
    await waitForStableFonts(page);

    await expect(page.getByRole("navigation", { name: "Atmospeak sections" })).toBeVisible();
    await expect(page.getByRole("heading", { name: /Hold, speak/ })).toBeVisible();
    await expect(page).toHaveScreenshot("hub-home-1000x660.png", {
      animations: "disabled",
      maxDiffPixels: 2,
    });
  });

  test("History matches the checked-in visual baseline and exposes real actions", async ({
    page,
  }) => {
    await page.goto("/?view=hub&fixture=hub");
    await page.getByRole("button", { name: "History" }).click();
    await waitForStableFonts(page);

    await expect(page.getByRole("heading", { name: /Said & set down/ })).toBeVisible();
    await expect(page.getByText("Letters")).toBeVisible();
    await page.getByRole("button", { expanded: false }).first().click();
    await expect(page.getByRole("button", { name: "Copy" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Delete" })).toBeVisible();
    await expect(page).toHaveScreenshot("hub-history-1000x660.png", {
      animations: "disabled",
      maxDiffPixels: 2,
    });
  });

  test("Advanced diagnostics remain inside Settings", async ({ page }) => {
    await page.goto("/?view=hub&fixture=hub");
    await page.getByRole("button", { name: "Settings" }).click();
    const disclosure = page.getByText("Advanced diagnostics", { exact: true });
    await expect(disclosure).toBeVisible();
    await disclosure.click();
    await expect(page.getByText("Advanced runtime", { exact: true })).toBeVisible();
  });

  test("dictionary and snippet records can be edited rather than duplicated", async ({ page }) => {
    await page.goto("/?view=hub&fixture=hub");
    await page.getByRole("button", { name: "Dictionary" }).click();
    await page.getByRole("button", { name: /Edit wind speak/i }).click();
    await expect(page.getByPlaceholder("heard phrase")).toHaveValue("wind speak");
    await expect(page.getByRole("button", { name: "Save" })).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();

    await page.getByRole("button", { name: "Snippets" }).click();
    await page.getByRole("button", { name: /Edit ship note/i }).click();
    await expect(page.getByPlaceholder("spoken trigger")).toHaveValue("ship note");
    await expect(page.getByRole("button", { name: "Save" })).toBeVisible();
  });
});

for (const deviceScaleFactor of [1.25, 1.5]) {
  test(`hub has no overflow at ${deviceScaleFactor * 100}% scale`, async ({ browser }) => {
    const context = await browser.newContext({
      viewport: { width: 1000, height: 660 },
      deviceScaleFactor,
    });
    const page = await context.newPage();
    await page.goto("/?view=hub&fixture=hub");
    await waitForStableFonts(page);
    const dimensions = await page.evaluate(() => ({
      documentWidth: document.documentElement.scrollWidth,
      viewportWidth: document.documentElement.clientWidth,
      documentHeight: document.documentElement.scrollHeight,
      viewportHeight: document.documentElement.clientHeight,
    }));
    expect(dimensions.documentWidth).toBeLessThanOrEqual(dimensions.viewportWidth);
    expect(dimensions.documentHeight).toBeLessThanOrEqual(dimensions.viewportHeight);
    await context.close();
  });
}

test("hub reflows at 360px without horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  await page.goto("/?view=hub&fixture=hub");
  await waitForStableFonts(page);
  const dimensions = await page.evaluate(() => ({
    documentWidth: document.documentElement.scrollWidth,
    viewportWidth: document.documentElement.clientWidth,
  }));
  expect(dimensions.documentWidth).toBe(dimensions.viewportWidth);
  await expect(page.getByRole("button", { name: "History" })).toBeVisible();
});

test("idle overlay owns no animation frame loop or waveform canvas", async ({ page }) => {
  await page.goto("/?view=overlay");
  await expect(page.getByRole("button", { name: /Atmospeak companion/i })).toBeVisible();
  const idleState = await page.evaluate(() => ({
    canvases: document.querySelectorAll("canvas").length,
    runningAnimations: document
      .getAnimations()
      .filter((animation) => animation.playState === "running").length,
  }));
  expect(idleState).toEqual({ canvases: 0, runningAnimations: 0 });
});
