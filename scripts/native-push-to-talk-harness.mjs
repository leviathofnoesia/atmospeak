import { execFileSync, spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const portFlag = process.argv.find((argument) => argument.startsWith("--port="));
const port = Number(portFlag?.split("=")[1]);
if (!Number.isInteger(port) || port <= 0) {
  throw new Error("Usage: node scripts/native-push-to-talk-harness.mjs --port=<port>");
}
const hotkeyFlag = process.argv.find((argument) => argument.startsWith("--hotkey="));

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const keyScript = join(root, "scripts", "send-native-keys.ps1");
const targetScript = join(root, "scripts", "native-injection-target.ps1");
const targetPath = join(tmpdir(), `atmospeak-ptt-target-${process.pid}.txt`);
const targetReadyPath = join(tmpdir(), `atmospeak-ptt-target-${process.pid}.ready`);
const fixtureMicrophone = "Atmospeak Test Audio Fixture";
const expectedPhrase = "The porcelain moon hums over the studio.";
const customHotkey = hotkeyFlag?.slice("--hotkey=".length) || "Ctrl+Alt+F12";
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

  const invoke = async (command, args = {}, timeoutMs = 120_000) => {
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
  if (initial.sessions.length !== 0) {
    throw new Error(`Isolated profile was not empty: ${initial.sessions.length} sessions.`);
  }
  const setupSettings = {
    ...initial.settings,
    microphoneName: fixtureMicrophone,
    hotkey: "Ctrl+Win",
    mode: "pushToTalk",
    autoInject: true,
    restoreClipboard: false,
    activeModelId: "base.en",
    startAtLogin: false,
    onboardingComplete: false,
    onboardingVersion: "",
    audioCalibration: null,
  };
  await invoke("save_settings", { settings: setupSettings });
  await invoke("start_sound_check", { deviceName: fixtureMicrophone });
  const soundCheck = await invoke(
    "finish_sound_check",
    { expectedPhrase },
    180_000,
  );
  if (!soundCheck.passed || soundCheck.asrBackend !== "host") {
    throw new Error(`Fixture sound check failed: ${JSON.stringify(soundCheck)}`);
  }

  const calibrated = await invoke("get_app_snapshot");
  const completeSettings = {
    ...calibrated.settings,
    hotkey: "Ctrl+Win",
    mode: "pushToTalk",
    autoInject: true,
    restoreClipboard: false,
    startAtLogin: false,
  };
  try {
    await invoke("complete_onboarding", { settings: completeSettings }, 120_000);
  } catch (error) {
    if (!String(error).includes("Execution context was destroyed")) throw error;
  }
  await page.waitForURL((url) => url.toString().includes("view=hub"), { timeout: 15_000 });
  await page.waitForFunction(
    () => typeof window.__TAURI_INTERNALS__?.invoke === "function",
    undefined,
    { timeout: 15_000 },
  );

  let completed;
  for (let attempt = 0; attempt < 5 && !completed; attempt += 1) {
    try {
      completed = await invoke("get_app_snapshot");
    } catch (error) {
      if (!String(error).includes("Execution context was destroyed")) throw error;
      await page.waitForTimeout(150);
    }
  }
  if (!completed) {
    throw new Error("The Hub WebView did not settle after setup navigation.");
  }
  if (!completed.settings.onboardingComplete) {
    throw new Error("Setup did not remain complete after the Hub navigation.");
  }
  const customSettings = {
    ...completed.settings,
    hotkey: customHotkey,
    mode: "pushToTalk",
    autoInject: true,
    restoreClipboard: false,
    startAtLogin: false,
  };
  // Reproduce the user-visible failure mode: an interrupted Settings
  // interaction left the hook in feedback-only capture/test state. Saving the
  // shortcut must atomically return it to normal dictation.
  await invoke("start_shortcut_capture", { currentHotkey: completed.settings.hotkey });
  await invoke("set_shortcut_test_active", { active: true });
  await invoke("save_settings", { settings: customSettings });
  const shortcut = await invoke("get_shortcut_status");
  if (!shortcut.registered || shortcut.hotkey !== customHotkey) {
    throw new Error(`Custom Settings hotkey was not active: ${JSON.stringify(shortcut)}`);
  }
  const overlayPage = nativePages().find((candidate) =>
    candidate.url().includes("view=overlay"),
  );
  if (!overlayPage) {
    throw new Error("Completing setup did not create the overlay WebView.");
  }
  await overlayPage.waitForFunction(
    (label) => document.body.textContent?.includes(`hold ${label}`),
    customHotkey,
    { timeout: 10_000 },
  );

  await page.evaluate(async () => {
    window.__atmospeakPttEvents = [];
    const subscribe = async (event) => {
      const callback = window.__TAURI_INTERNALS__.transformCallback(({ payload }) => {
        window.__atmospeakPttEvents.push({ event, payload, receivedAt: Date.now() });
      });
      await window.__TAURI_INTERNALS__.invoke("plugin:event|listen", {
        event,
        target: { kind: "Any" },
        handler: callback,
      });
    };
    await subscribe("atmospeak://native-dictation");
    await subscribe("atmospeak://stage-metrics");
  });

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
    if (focus) {
      args.push("-FocusProcessId", String(nativeTarget.pid));
    }
    execFileSync("powershell.exe", args, {
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
  };
  const events = () => page.evaluate(() => window.__atmospeakPttEvents);
  const waitForPhase = async (phase, timeoutMs) => {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const match = (await events()).find(
        ({ event, payload }) =>
          event === "atmospeak://native-dictation" && payload.phase === phase,
      );
      if (match) return match.payload;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error(
      `Timed out waiting for native phase ${phase}: ${JSON.stringify(await events())}`,
    );
  };

  sendKeys("down", customHotkey, true);
  await waitForPhase("listening", 5_000);
  await new Promise((resolve) => setTimeout(resolve, 400));
  sendKeys("up", customHotkey);
  const pasted = await waitForPhase("pasted", 120_000);
  if (!pasted.result?.session?.injected || !pasted.result?.injection?.injected) {
    throw new Error(`Release did not inject successfully: ${JSON.stringify(pasted)}`);
  }

  const metricsEvent = (await events()).find(
    ({ event }) => event === "atmospeak://stage-metrics",
  );
  const metrics = metricsEvent?.payload ?? pasted.metrics;
  if (metrics?.asrBackend !== "host") {
    throw new Error(`Push-to-talk did not use the resident host: ${JSON.stringify(metrics)}`);
  }

  const expectedText = pasted.result.session.cleanedText.trim();
  if (!pasted.result.injection?.restoredTarget) {
    throw new Error(
      `Atmospeak did not restore the captured target before paste: ${JSON.stringify(
        pasted.result.injection,
      )}`,
    );
  }
  let targetText = "";
  const saveDeadline = Date.now() + 5_000;
  while (Date.now() < saveDeadline) {
    targetText = readFileSync(targetPath, "utf8").replace(/^\uFEFF/, "").trim();
    if (targetText) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  if (targetText !== expectedText) {
    throw new Error(
      `Native target did not contain exactly one injected transcript: ${JSON.stringify({
        expectedText,
        targetText,
        injection: pasted.result.injection,
      })}`,
    );
  }

  const finalSnapshot = await invoke("get_app_snapshot");
  if (finalSnapshot.settings.hotkey !== customHotkey) {
    throw new Error(
      `Custom shortcut was not persisted: ${JSON.stringify(finalSnapshot.settings)}`,
    );
  }
  if (finalSnapshot.sessions.length !== 1 || !finalSnapshot.sessions[0].injected) {
    throw new Error(
      `Release created the wrong session count: ${JSON.stringify(finalSnapshot.sessions)}`,
    );
  }

  console.log(
    JSON.stringify({
      passed: true,
      hotkey: customHotkey,
      persistedHotkey: finalSnapshot.settings.hotkey,
      orbHotkeyLabelVerified: true,
      staleInteractionStateCleared: true,
      mode: customSettings.mode,
      releaseAutoInjected: true,
      sessionId: pasted.result.session.id,
      transcript: expectedText,
      targetText,
      asrBackend: metrics.asrBackend,
      captureStopMs: metrics.captureStopMs,
      asrMs: metrics.asrMs,
      injectMs: metrics.injectMs,
      totalMs: metrics.totalMs,
      soundCheck,
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
