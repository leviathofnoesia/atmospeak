import { expect, test } from "@playwright/test";

test("loads the Wind Speak hub in browser mock mode", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Desktop dictation instrument" })).toBeVisible();
  await expect(page.getByText("Bundled runtime")).toBeVisible();
  await page.getByRole("button", { name: "Enter hub" }).click();

  await expect(page.getByRole("heading", { name: "Local dictation console" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start dictation" })).toBeVisible();
});
