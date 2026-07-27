import { chromium } from "playwright";

const portFlag = process.argv.find((argument) => argument.startsWith("--port="));
const expectedFlag = process.argv.find((argument) => argument.startsWith("--expect="));
const port = Number(portFlag?.split("=")[1]);
const expected = expectedFlag?.split("=")[1] ?? "setup";

if (!Number.isInteger(port) || port <= 0) {
  throw new Error("Usage: bun scripts/native-webview-harness.mjs --port=<port> --expect=setup");
}

let browser;
let lastError;
const deadline = Date.now() + 20_000;
while (!browser && Date.now() < deadline) {
  try {
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1_000 });
  } catch (error) {
    lastError = error;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}
if (!browser) {
  throw new Error(`WebView2 debugging did not open on port ${port}: ${lastError}`);
}

const pages = browser.contexts().flatMap((context) => context.pages());
if (pages.length !== 1) {
  throw new Error(`Expected exactly one native WebView, found ${pages.length}.`);
}

const page = pages[0];
if (page.url() === "about:blank") {
  await page.waitForURL((url) => url.toString() !== "about:blank", { timeout: 10_000 });
}
await page.waitForLoadState("domcontentloaded");
const url = page.url();

if (expected === "setup") {
  if (!url.includes("view=setup")) {
    throw new Error(`Expected setup route, received ${url}`);
  }
  await page.getByText("Welcome", { exact: true }).first().waitFor({ timeout: 10_000 });
  const bodyText = await page.locator("body").innerText();
  if (!bodyText.includes("Welcome") || !bodyText.includes("Speak. It listens.")) {
    throw new Error("Native setup DOM is missing the supplied Welcome content.");
  }
  if (/skip setup/i.test(bodyText)) {
    throw new Error("Native setup DOM still exposes a setup bypass.");
  }

  const overlayAttempt = await page.evaluate(async () => {
    try {
      await window.__TAURI_INTERNALS__.invoke("show_overlay_window");
      return { rejected: false, message: "resolved" };
    } catch (error) {
      return { rejected: true, message: String(error) };
    }
  });
  if (!overlayAttempt.rejected || !/finish microphone setup/i.test(overlayAttempt.message)) {
    throw new Error(`Incomplete setup did not reject overlay creation: ${JSON.stringify(overlayAttempt)}`);
  }
  const pagesAfterAttempt = browser.contexts().flatMap((context) => context.pages());
  if (pagesAfterAttempt.length !== 1) {
    throw new Error("Failed setup created an overlay WebView.");
  }
}

console.log(
  JSON.stringify({
    passed: true,
    expected,
    url,
    pageCount: pages.length,
    welcomeVisible:
      expected === "setup"
        ? await page.getByText("Welcome", { exact: true }).first().isVisible()
        : false,
    overlayAbsent: browser.contexts().flatMap((context) => context.pages()).length === 1,
  }),
);
process.exit(0);
