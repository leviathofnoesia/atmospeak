import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium } from "playwright";

const portFlag = process.argv.find((argument) => argument.startsWith("--port="));
const port = Number(portFlag?.split("=")[1]);
if (!Number.isInteger(port) || port <= 0) {
  throw new Error("Usage: node scripts/native-shortcut-harness.mjs --port=<port>");
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
const consoleProblems = [];
page.on("console", (message) => {
  if (message.type() === "error" || message.type() === "warning") {
    consoleProblems.push(`${message.type()}: ${message.text()}`);
  }
});
page.on("pageerror", (error) => consoleProblems.push(`pageerror: ${error.message}`));
await page.waitForLoadState("domcontentloaded");
await page.goto("http://localhost:1420/?view=setup&fixture=shortcut");
await page.waitForLoadState("domcontentloaded");
if (!page.url().includes("view=setup")) {
  throw new Error(`Unexpected native page URL: ${page.url()}`);
}
await page.getByText("Welcome", { exact: true }).first().waitFor({ timeout: 10_000 });

await page.evaluate(async () => {
  window.__atmospeakShortcutProbe = [];
  const subscribe = async (event) => {
    const callback = window.__TAURI_INTERNALS__.transformCallback(({ payload }) => {
      window.__atmospeakShortcutProbe.push({ event, payload, receivedAt: Date.now() });
    });
    await window.__TAURI_INTERNALS__.invoke("plugin:event|listen", {
      event,
      target: { kind: "Any" },
      handler: callback,
    });
  };
  await subscribe("atmospeak://shortcut-key");
  await subscribe("atmospeak://shortcut-capture");
  await subscribe("wind-speak://shortcut");
});

function sendVirtualKeys(keys, release = false) {
  const ordered = release ? [...keys].reverse() : keys;
  const flags = release ? 2 : 0;
  const calls = ordered
    .map(
      (key) =>
        `[NativeKeys]::keybd_event([byte]${key}, 0, ${flags}, [UIntPtr]::Zero)`,
    )
    .join("; ");
  const script = [
    "Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class NativeKeys { [DllImport(\"user32.dll\")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra); }'",
    calls,
    "Start-Sleep -Milliseconds 100",
  ].join("; ");
  execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script]);
}

function injectVirtualKeys(keys) {
  sendVirtualKeys(keys);
  sendVirtualKeys(keys, true);
}

async function resetProbe() {
  await page.evaluate(() => {
    window.__atmospeakShortcutProbe = [];
  });
}

async function probe() {
  await new Promise((resolve) => setTimeout(resolve, 250));
  return page.evaluate(() => window.__atmospeakShortcutProbe);
}

const invoke = (command, args = {}) =>
  page.evaluate(
    ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
    { command, args },
  );

// Exercise the rendered onboarding flow, not just the command API.
await page.getByRole("button", { name: /begin/i }).click();
await page.getByRole("button", { name: /^continue$/i }).click();
await page.getByRole("button", { name: /^continue$/i }).click();
await page.getByRole("button", { name: /record shortcut/i }).click();
await page.getByRole("button", { name: /recording keys/i }).waitFor();
await resetProbe();
const physicalKeys = [
  { code: 0xA2, label: "Ctrl" },
  { code: 0xA4, label: "Alt" },
  { code: 0x4B, label: "K" },
];
const paintChecks = [];
for (let index = 0; index < physicalKeys.length; index += 1) {
  sendVirtualKeys([physicalKeys[index].code]);
  const expected = physicalKeys.slice(0, index + 1).map(({ label }) => label);
  const lit = await page.locator("kbd.is-down").allTextContents();
  paintChecks.push({ expected, lit });
  if (JSON.stringify(lit) !== JSON.stringify(expected)) {
    throw new Error(
      `Keyboard-tester failed after ${physicalKeys[index].label} down: ${JSON.stringify(lit)}`,
    );
  }
}
const litKeys = paintChecks.at(-1).lit;
const screenshotPath = join(tmpdir(), "atmospeak-shortcut-realtime.png");
await page.screenshot({ path: screenshotPath });
for (const { code } of [...physicalKeys].reverse()) {
  sendVirtualKeys([code], true);
}
await page.getByText(/Ctrl\+Alt\+K recorded/i).first().waitFor();
const captureEvents = await probe();
const captureStates = captureEvents.filter(
  ({ event }) => event === "atmospeak://shortcut-capture",
);
const captureMissing = ["Ctrl", "Alt", "K"].filter(
  (key) => !captureStates.some(({ payload }) => payload.keys.includes(key)),
);
const completedCapture = captureStates.find(
  ({ payload }) => payload.completed === "Ctrl+Alt+K",
);
if (!completedCapture) captureMissing.push("completed chord");
const eventLatencies = captureStates
  .map(({ payload, receivedAt }) => receivedAt - Number(payload.timestampMs))
  .filter((latency) => latency >= 0);
const maxEventLatencyMs = Math.max(...eventLatencies);

await page.getByRole("button", { name: /test selected/i }).click();
sendVirtualKeys([0xA2, 0xA4, 0x4B]);
const testLitKeys = await page.locator("kbd.is-down").allTextContents();
if (JSON.stringify(testLitKeys) !== JSON.stringify(["Ctrl", "Alt", "K"])) {
  throw new Error(`Exact-test lighting was wrong: ${JSON.stringify(testLitKeys)}`);
}
sendVirtualKeys([0xA2, 0xA4, 0x4B], true);
await page.getByText(/Ctrl\+Alt\+K detected by the desktop runtime/i).waitFor();
if (await page.getByRole("button", { name: /^continue$/i }).isDisabled()) {
  throw new Error("Continue remained disabled after the exact native chord passed.");
}

async function verifyChord(label, keys) {
  await resetProbe();
  const status = await invoke("register_setup_shortcut", { hotkey: label });
  if (!status.registered || status.hotkey !== label) {
    throw new Error(`Runtime did not register exact chord ${label}: ${JSON.stringify(status)}`);
  }
  injectVirtualKeys(keys);
  const events = await probe();
  const signals = events
    .filter(({ event }) => event === "wind-speak://shortcut")
    .map(({ payload }) => payload);
  if (!signals.includes("pressed") || !signals.includes("released")) {
    throw new Error(`${label} did not produce pressed/released: ${JSON.stringify(events)}`);
  }
  await invoke("set_shortcut_test_active", { active: false });
  return { label, signals };
}

const verified = [
  await verifyChord("Ctrl+Shift+F12", [0xA2, 0xA0, 0x7B]),
  await verifyChord("Ctrl+Win", [0xA2, 0x5B]),
];

if (captureMissing.length) {
  throw new Error(
    `Native capture missed ${captureMissing.join(", ")}: ${JSON.stringify(captureEvents)}; verification=${JSON.stringify(verified)}`,
  );
}
if (consoleProblems.length) {
  throw new Error(`Native WebView console was not clean: ${JSON.stringify(consoleProblems)}`);
}

console.log(
  JSON.stringify({
    passed: true,
    captureKeys: ["Ctrl", "Alt", "K"],
    litKeys,
    paintChecks,
    maxEventLatencyMs,
    screenshotPath,
    consoleProblems,
    exactUiTest: true,
    verified,
  }),
);
process.exit(0);
