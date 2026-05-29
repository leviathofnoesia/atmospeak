import { expect, test } from "@playwright/test";

test("loads the Wind Speak hub in browser mock mode", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Desktop dictation instrument" })).toBeVisible();
  await expect(page.getByText("Bundled runtime")).toBeVisible();
  await page.getByLabel("Shortcut").selectOption("Ctrl+Alt+Space");
  await page.getByLabel("Capture mode").selectOption("toggle");
  await page.getByRole("button", { name: "Enter hub" }).click();

  await expect(page.getByRole("heading", { name: "Local dictation console" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start dictation" })).toBeVisible();
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByLabel("Shortcut")).toHaveValue("Ctrl+Alt+Space");
  await expect(page.getByLabel("Mode")).toHaveValue("toggle");
});
