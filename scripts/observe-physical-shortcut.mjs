import { chromium } from "playwright";

const port = Number(process.argv.find((arg) => arg.startsWith("--port="))?.split("=")[1]);
const mode = process.argv.find((arg) => arg.startsWith("--mode="))?.split("=")[1] ?? "prepare";
if (!port) throw new Error("Pass --port=<WebView2 debugging port>");

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser
  .contexts()
  .flatMap((context) => context.pages())
  .find((candidate) => !candidate.url().startsWith("edge://"));
if (!page) throw new Error("Atmospeak WebView was not found.");

if (mode === "prepare") {
  await page.goto("http://localhost:1420/?view=setup&fixture=shortcut");
  await page.getByText("Welcome", { exact: true }).first().waitFor();
  await page.getByRole("button", { name: /begin/i }).click();
  await page.getByRole("button", { name: /^continue$/i }).click();
  await page.getByRole("button", { name: /^continue$/i }).click();
  await page.evaluate(() => {
    window.__physicalShortcutEvents = [];
    const record = (kind, detail = {}) => {
      window.__physicalShortcutEvents.push({
        at: performance.now(),
        kind,
        focused: document.hasFocus(),
        activeElement: document.activeElement?.textContent?.trim() ?? "",
        lit: [...document.querySelectorAll("kbd.is-down")].map((key) => key.textContent),
        ...detail,
      });
    };
    window.addEventListener(
      "keydown",
      (event) => record("keydown", { code: event.code, key: event.key, repeat: event.repeat }),
      true,
    );
    window.addEventListener(
      "keyup",
      (event) => record("keyup", { code: event.code, key: event.key }),
      true,
    );
    window.addEventListener("focus", () => record("focus"));
    window.addEventListener("blur", () => record("blur"));
    new MutationObserver(() => record("paint")).observe(document.querySelector(".ob-keys"), {
      attributes: true,
      childList: true,
      subtree: true,
    });
    const subscribe = async (event) => {
      const callback = window.__TAURI_INTERNALS__.transformCallback(({ payload }) => {
        record("native", { event, payload });
      });
      await window.__TAURI_INTERNALS__.invoke("plugin:event|listen", {
        event,
        target: { kind: "Any" },
        handler: callback,
      });
    };
    void Promise.all([
      subscribe("atmospeak://shortcut-key"),
      subscribe("wind-speak://shortcut"),
      subscribe("wind-speak://shortcut-status"),
    ]);
    record("ready");
  });
  console.log(JSON.stringify({ ready: true, title: await page.title(), url: page.url() }));
  process.exit(0);
}

const deadline = Date.now() + 90_000;
let emitted = 0;
while (Date.now() < deadline) {
  const events = await page.evaluate(() => window.__physicalShortcutEvents ?? []);
  for (const event of events.slice(emitted)) {
    console.log(JSON.stringify(event));
  }
  emitted = events.length;
  if (events.some((event) => event.kind === "keyup")) {
    console.log(
      JSON.stringify({
        complete: true,
        bodyStatus: await page.locator(".ob-keyhint").innerText(),
        selectedKeys: await page.locator(".ob-keys kbd").allTextContents(),
      }),
    );
    process.exit(0);
  }
  await new Promise((resolve) => setTimeout(resolve, 100));
}
throw new Error("No physical key-up reached the focused Atmospeak WebView within 90 seconds.");
