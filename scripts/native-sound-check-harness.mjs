import { chromium } from "playwright";

const portFlag = process.argv.find((argument) => argument.startsWith("--port="));
const deviceFlag = process.argv.find((argument) => argument.startsWith("--device="));
const port = Number(portFlag?.split("=")[1]);
const preferredDevice = deviceFlag?.slice("--device=".length) ?? "Elgato Wave:3";
if (!Number.isInteger(port) || port <= 0) {
  throw new Error(
    "Usage: node scripts/native-sound-check-harness.mjs --port=<port> [--device=Elgato Wave:3]",
  );
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

const pages = browser
  .contexts()
  .flatMap((context) => context.pages())
  .filter((page) => !page.url().startsWith("edge://"));
if (pages.length !== 1) {
  throw new Error(`Expected one native setup WebView, found ${pages.length}.`);
}
const page = pages[0];
await page.waitForLoadState("domcontentloaded");
await page.evaluate(() => {
  window.__atmospeakHeartbeat = 0;
  window.__atmospeakHeartbeatTimer = window.setInterval(() => {
    window.__atmospeakHeartbeat += 1;
  }, 50);
});

const invoke = async (command, args = {}, timeoutMs = 30_000) => {
  let timeout;
  try {
    return await Promise.race([
      page.evaluate(
        ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
        { command, args },
      ),
      new Promise((_, reject) => {
        timeout = setTimeout(() => {
          reject(
            new Error(`Native command "${command}" did not finish within ${timeoutMs}ms.`),
          );
        }, timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timeout);
  }
};
const microphones = await invoke("list_microphones");
const microphone =
  microphones.find((candidate) =>
    candidate.name.toLocaleLowerCase().includes(preferredDevice.toLocaleLowerCase()),
  ) ??
  microphones.find((candidate) => candidate.is_default) ??
  microphones[0];
if (!microphone) {
  throw new Error("No microphone was available to the native sound-check harness.");
}

await invoke("start_sound_check", { deviceName: microphone.name });
await new Promise((resolve) => setTimeout(resolve, 2_500));
const heartbeatBeforeFinish = await page.evaluate(() => window.__atmospeakHeartbeat);
const finishStartedAt = Date.now();
const result = await invoke("finish_sound_check", {
  expectedPhrase: "The porcelain moon hums over the studio.",
}, 120_000);
const finishElapsedMs = Date.now() - finishStartedAt;
const heartbeatAfterFinish = await page.evaluate(() => window.__atmospeakHeartbeat);
const minimumHeartbeats = Math.max(1, Math.floor(finishElapsedMs / 200));
if (heartbeatAfterFinish - heartbeatBeforeFinish < minimumHeartbeats) {
  throw new Error(
    `The WebView stopped responding during sound-check finish: ${JSON.stringify({
      finishElapsedMs,
      heartbeatBeforeFinish,
      heartbeatAfterFinish,
    })}`,
  );
}
if (result.deviceName !== microphone.name) {
  throw new Error(`Sound check used the wrong microphone: ${JSON.stringify(result)}`);
}
if (result.durationMs < 2_000 || result.durationMs > 12_000) {
  throw new Error(`Sound-check capture duration was invalid: ${JSON.stringify(result)}`);
}

await page.evaluate(() => window.clearInterval(window.__atmospeakHeartbeatTimer));
console.log(
  JSON.stringify({
    passed: true,
    deviceName: microphone.name,
    finishElapsedMs,
    heartbeatDelta: heartbeatAfterFinish - heartbeatBeforeFinish,
    soundCheckPassed: result.passed,
    failureCode: result.failureCode,
    rmsDbfs: result.rmsDbfs,
    peakDbfs: result.peakDbfs,
    snrDb: result.snrDb,
    asrBackend: result.asrBackend,
    asrMs: result.asrMs,
  }),
);
process.exit(0);
