import { expect, test } from "@playwright/test";

test("loads onboarding, enters the redesigned hub, and exercises browser mock actions", async ({ page }) => {
  await page.goto("/");

  await expect(
    page.getByRole("heading", { name: "Speak. It listens. It sets the words down." }),
  ).toBeVisible();
  await expect(page.getByText("First run")).toBeVisible();

  await page.getByRole("button", { name: "Skip setup" }).click();
  await page.getByRole("button", { name: /Enter Atmospeak/ }).click();

  await expect(page.getByRole("navigation", { name: "Atmospeak sections" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start dictation" })).toBeVisible();

  await page.getByRole("button", { name: "Start dictation" }).click();
  await expect(page.getByText(/Recording from/)).toBeVisible();
  await page.getByRole("button", { name: "Start dictation" }).click();

  await expect(page.getByRole("heading", { name: "Transcript history" })).toBeVisible();
  await page.getByRole("button", { name: "Copy transcript" }).click();
  await expect(page.getByText("Transcript copied to clipboard.")).toBeVisible();
  await page.getByRole("button", { name: "Paste transcript again" }).click();
  await expect(page.getByText("Mock transcript copied to the focused application.")).toBeVisible();

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByLabel("Shortcut")).toHaveValue("Ctrl+Win");
  await page.getByLabel("Shortcut").selectOption("Ctrl+Alt+Space");
  await expect(page.getByLabel("Shortcut")).toHaveValue("Ctrl+Alt+Space");
  await page.locator('label:has(span:text-is("Mode")) select').selectOption("toggle");
  await page.getByRole("button", { name: "Pause shortcuts" }).click();
  await expect(page.getByRole("button", { name: "Resume shortcuts" })).toBeVisible();
  await page.getByRole("button", { name: "Test active shortcut" }).click();
  await expect(page.getByText("Shortcuts are paused. Resume shortcuts and test again.")).toBeVisible();

  await page.getByRole("button", { name: "Run onboarding" }).click();
  await expect(
    page.getByRole("heading", { name: "Speak. It listens. It sets the words down." }),
  ).toBeVisible();
});

test("loads the redesigned floating companion overlay in browser mock mode", async ({ page }) => {
  await page.goto("/?view=overlay");

  await expect(
    page.getByRole("button", { name: /Atmospeak companion/i }),
  ).toBeVisible();
  await expect(page.getByText(/hold/i)).toBeVisible();
});
