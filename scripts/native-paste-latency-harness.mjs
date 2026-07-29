import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { spawn } from "node:child_process";

const portFlag = process.argv.find((argument) => argument.startsWith("--port="));
const port = Number(portFlag?.split("=")[1]);
if (!Number.isInteger(port) || port <= 0) {
  throw new Error("Usage: node scripts/native-paste-latency-harness.mjs --port=<port>");
}

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const targetScript = join(root, "scripts", "native-injection-target.ps1");
const targetPath = join(tmpdir(), `atmospeak-paste-target-${process.pid}.txt`);
const targetReadyPath = join(tmpdir(), `atmospeak-paste-target-${process.pid}.ready`);
const pasteText = "Atmospeak paste latency probe.";
/** Wall time from inject_text invoke until target text appears (includes IPC). */
const PASTE_VISIBLE_MS_BUDGET = 300;
writeFileSync(targetPath, "", "utf8");

let browser;
let nativeTarget;
try {
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

  const nativePages = () =>
    browser
      .contexts()
      .flatMap((context) => context.pages())
      .filter((candidate) => !candidate.url().startsWith("edge://"));
  const page = nativePages()[0];
  if (!page) throw new Error("Atmospeak did not create a native WebView.");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForFunction(
    () => typeof window.__TAURI_INTERNALS__?.invoke === "function",
    undefined,
    { timeout: 15_000 },
  );

  const invoke = async (command, args = {}, timeoutMs = 30_000) => {
    let timeout;
    try {
      return await Promise.race([
        page.evaluate(
          ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
          { command, args },
        ),
        new Promise((_, reject) => {
          timeout = setTimeout(
            () => reject(new Error(`Native command "${command}" exceeded ${timeoutMs}ms.`)),
            timeoutMs,
          );
        }),
      ]);
    } finally {
      clearTimeout(timeout);
    }
  };

  const initial = await invoke("get_app_snapshot");
  const settings = {
    ...initial.settings,
    autoInject: true,
    restoreClipboard: true,
    startAtLogin: false,
    onboardingComplete: true,
    onboardingVersion: initial.settings.onboardingVersion || "desktop-parity-v5",
  };
  await invoke("save_settings", { settings });

  nativeTarget = spawn(
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
      targetReadyPath,
    ],
    {
      windowsHide: false,
      stdio: "ignore",
    },
  );
  const targetDeadline = Date.now() + 10_000;
  while (!existsSync(targetReadyPath) && Date.now() < targetDeadline) {
    if (nativeTarget.exitCode != null) {
      throw new Error(`Native injection target exited with code ${nativeTarget.exitCode}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  if (!existsSync(targetReadyPath)) {
    throw new Error("Native injection target did not become ready.");
  }
  const targetHwnd = readFileSync(targetReadyPath, "utf8").trim();
  if (!/^-?\d+$/.test(targetHwnd) || targetHwnd === "0") {
    throw new Error(`Native injection target HWND was invalid: ${targetHwnd}`);
  }

  // Give Atmospeak a recent-input path to SetForegroundWindow by focusing the
  // target from a helper that just received synthetic key input.
  const { execFileSync } = await import("node:child_process");
  const keyScript = join(root, "scripts", "send-native-keys.ps1");
  execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      keyScript,
      "-Action",
      "press",
      "-Keys",
      "Ctrl",
      "-FocusProcessId",
      String(nativeTarget.pid),
    ],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
  );

  const started = Date.now();
  const injection = await invoke("inject_text", {
    text: pasteText,
    targetHwnd,
  });
  if (!injection?.injected) {
    throw new Error(`inject_text soft-failed: ${JSON.stringify(injection)}`);
  }
  if (!injection.restoredTarget) {
    throw new Error(
      `inject_text did not restore the paste target: ${JSON.stringify(injection)}`,
    );
  }

  let targetText = "";
  const saveDeadline = Date.now() + 2_000;
  while (Date.now() < saveDeadline) {
    targetText = readFileSync(targetPath, "utf8").replace(/^\uFEFF/, "").trim();
    if (targetText === pasteText) break;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  const pasteVisibleMs = Date.now() - started;
  if (targetText !== pasteText) {
    throw new Error(
      `Paste target mismatch: ${JSON.stringify({ pasteText, targetText, injection })}`,
    );
  }
  if (pasteVisibleMs > PASTE_VISIBLE_MS_BUDGET) {
    throw new Error(
      `Paste-visible latency SLO failed: ${JSON.stringify({
        pasteVisibleMs,
        budgetMs: PASTE_VISIBLE_MS_BUDGET,
        injection,
      })}`,
    );
  }

  console.log(
    JSON.stringify({
      passed: true,
      pasteVisibleMs,
      budgetMs: PASTE_VISIBLE_MS_BUDGET,
      restoredClipboard: injection.restoredClipboard,
      restoredTarget: injection.restoredTarget,
      targetText,
    }),
  );
} finally {
  if (nativeTarget && nativeTarget.exitCode == null) {
    nativeTarget.kill();
    await Promise.race([
      new Promise((resolve) => nativeTarget.once("exit", resolve)),
      new Promise((resolve) => setTimeout(resolve, 2_000)),
    ]);
  }
  rmSync(targetPath, { force: true });
  rmSync(targetReadyPath, { force: true });
  await browser?.close().catch(() => {});
}
