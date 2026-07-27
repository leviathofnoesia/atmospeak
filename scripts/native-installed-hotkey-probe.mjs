import { execFileSync, spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const portFlag = process.argv.find((argument) => argument.startsWith("--port="));
const hotkeyFlag = process.argv.find((argument) => argument.startsWith("--hotkey="));
const focusPidFlag = process.argv.find((argument) => argument.startsWith("--focus-pid="));
const port = Number(portFlag?.slice("--port=".length));
const hotkey = hotkeyFlag?.slice("--hotkey=".length);
const focusPid = focusPidFlag ? Number(focusPidFlag.slice("--focus-pid=".length)) : null;
if (!Number.isInteger(port) || port <= 0 || !hotkey) {
  throw new Error(
    "Usage: node scripts/native-installed-hotkey-probe.mjs --port=<port> --hotkey=<label> [--focus-pid=<pid>]",
  );
}
if (focusPid !== null && (!Number.isInteger(focusPid) || focusPid <= 0)) {
  throw new Error("--focus-pid must identify a running foreground application.");
}

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const keyScript = join(root, "scripts", "send-native-keys.ps1");
const targetScript = join(root, "scripts", "native-injection-target.ps1");
const targetPath = join(tmpdir(), `atmospeak-installed-probe-${process.pid}.txt`);
const readyPath = join(tmpdir(), `atmospeak-installed-probe-${process.pid}.ready`);
writeFileSync(targetPath, "", "utf8");

let browser;
let target;
let targetPid = focusPid;
const heldKeys = [];
const hotkeyParts = hotkey.split("+");
const triggerKey = hotkeyParts.at(-1);
const leadKeys = hotkeyParts.slice(0, -1).join("+");
const sendKeys = (action, keys, focus = false) => {
  const args = [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    keyScript,
    "-Action",
    action,
    "-Keys",
    keys,
  ];
  if (focus) args.push("-FocusProcessId", String(targetPid));
  execFileSync("powershell.exe", args, {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    timeout: 15_000,
  });
};

try {
  const deadline = Date.now() + 20_000;
  let lastError;
  while (!browser && Date.now() < deadline) {
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
        timeout: 1_000,
      });
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  }
  if (!browser) {
    throw new Error(`Installed WebView debugging did not open: ${lastError}`);
  }

  const pages = browser.contexts().flatMap((context) => context.pages());
  const overlay = pages.find((page) => page.url().includes("view=overlay"));
  if (!overlay) throw new Error("Installed app did not create its overlay WebView.");
  await overlay.waitForFunction(
    (expected) => document.body.textContent?.includes(`hold ${expected}`),
    hotkey,
    { timeout: 10_000 },
  );
  await overlay.waitForFunction(
    () => typeof window.__TAURI_INTERNALS__?.invoke === "function",
    undefined,
    { timeout: 10_000 },
  );

  await overlay.evaluate(async () => {
    window.__atmospeakInstalledProbe = [];
    const callback = window.__TAURI_INTERNALS__.transformCallback(({ payload }) => {
      window.__atmospeakInstalledProbe.push(payload);
    });
    await window.__TAURI_INTERNALS__.invoke("plugin:event|listen", {
      event: "atmospeak://native-dictation",
      target: { kind: "Any" },
      handler: callback,
    });
  });

  if (targetPid === null) {
    target = spawn(
      "powershell.exe",
      [
        "-NoProfile",
        "-STA",
        "-WindowStyle",
        "Hidden",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        targetScript,
        "-OutputPath",
        targetPath,
        "-ReadyPath",
        readyPath,
      ],
      { windowsHide: false, stdio: "ignore" },
    );
    targetPid = target.pid;
    const targetDeadline = Date.now() + 10_000;
    while (!existsSync(readyPath) && Date.now() < targetDeadline) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    if (!existsSync(readyPath)) throw new Error("External typing target did not open.");
  }

  sendKeys("down", leadKeys, true);
  heldKeys.push(leadKeys);
  await overlay.waitForFunction(
    () =>
      document.querySelector(".dock")?.hasAttribute("data-armed") &&
      document.querySelector(".dock")?.getAttribute("data-state") === "rest",
    undefined,
    { timeout: 2_000 },
  );

  sendKeys("down", triggerKey);
  heldKeys.push(triggerKey);

  await overlay.waitForFunction(
    () =>
      window.__atmospeakInstalledProbe?.some((event) => event.phase === "listening") &&
      document.querySelector(".dock")?.getAttribute("data-state") === "listening",
    undefined,
    { timeout: 5_000 },
  );

  await overlay.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("handle_dictation_action", {
      action: "cancel",
    }),
  );
  sendKeys("up", triggerKey);
  heldKeys.pop();
  sendKeys("up", leadKeys);
  heldKeys.pop();

  await overlay.waitForFunction(
    () => document.querySelector(".dock")?.getAttribute("data-state") === "rest",
    undefined,
    { timeout: 5_000 },
  );
  const targetText = focusPid === null ? readFileSync(targetPath, "utf8").trim() : "";
  if (targetText) {
    throw new Error(`Cancelled probe unexpectedly injected text: ${targetText}`);
  }
  console.log(
    JSON.stringify({
      passed: true,
      build: "installed-release",
      focusedApplication:
        focusPid === null ? "external native text editor" : `existing process ${focusPid}`,
      hotkey,
      orbLabelVerified: true,
      globalLeadKeyFeedbackVerified: true,
      nativeListeningEventVerified: true,
      orbMorphedToListening: true,
      cancelledWithoutTranscriptionOrInjection: true,
    }),
  );
} finally {
  while (heldKeys.length) {
    const keys = heldKeys.pop();
    try {
      sendKeys("up", keys);
    } catch {
      // The parent cleanup still terminates the isolated app process.
    }
  }
  if (target && target.exitCode == null) {
    target.kill();
    await Promise.race([
      new Promise((resolve) => target.once("exit", resolve)),
      new Promise((resolve) => setTimeout(resolve, 2_000)),
    ]);
  }
  rmSync(targetPath, { force: true });
  rmSync(readyPath, { force: true });
  await Promise.race([
    browser?.close().catch(() => {}),
    new Promise((resolve) => setTimeout(resolve, 2_000)),
  ]);
}
