import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

const args = Object.fromEntries(process.argv.slice(2).map((value) => {
  const split = value.indexOf("=");
  return [value.slice(2, split), value.slice(split + 1)];
}));
if (args["allow-desktop-input"] !== "true") {
  throw new Error("This interactive tool changes keyboard focus, sends keys and replaces the clipboard. Run only after explicit permission for an uninterrupted desktop session, using --allow-desktop-input=true.");
}
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const required = (key) => {
  const value = args[key];
  if (!value || !isAbsolute(value) || !/^E:[\\/]/i.test(value)) throw new Error(`--${key}=<absolute E: path> is required`);
  return resolve(value);
};
const appExe = required("app");
const recording = required("recording");
const runRoot = required("run-root");
const seedDatabase = required("seed-database");
const runs = Number(args.runs ?? 5);
if (!Number.isInteger(runs) || runs < 1 || runs > 30) throw new Error("--runs must be 1–30");
if (![appExe, recording, seedDatabase].every(existsSync)) throw new Error("Input file is missing");
const hotkey = args.hotkey ?? "Ctrl+Shift+F12";
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const wave = readFileSync(recording);
let rate, channels, bits, audioBytes;
for (let offset = 12; offset + 8 <= wave.length;) {
  const size = wave.readUInt32LE(offset + 4);
  const kind = wave.toString("ascii", offset, offset + 4);
  if (kind === "fmt ") {
    channels = wave.readUInt16LE(offset + 10);
    rate = wave.readUInt32LE(offset + 12);
    bits = wave.readUInt16LE(offset + 22);
  }
  if (kind === "data") audioBytes = size;
  offset += 8 + size + (size & 1);
}
if (rate !== 16000 || channels !== 1 || bits !== 16 || !audioBytes) throw new Error("Use an existing mono 16 kHz PCM16 recording");
const audioMs = audioBytes / 32;
mkdirSync(runRoot, { recursive: true });
const profile = join(runRoot, "profile");
mkdirSync(profile, { recursive: true });
const databasePath = join(profile, "wind-speak.sqlite3");
if (existsSync(databasePath)) throw new Error("Use a fresh run-root; never overwrite a previous benchmark");
copyFileSync(seedDatabase, databasePath);
const database = new DatabaseSync(databasePath);
database.exec("PRAGMA busy_timeout=2000");
const originalSettings = JSON.parse(database.prepare("select value from settings where key='app'").get().value);
const settings = { ...originalSettings, hotkey, voiceEditHotkey: "", applicationProfiles: [], mode: "pushToTalk", autoInject: true, autoPolish: false, restoreClipboard: true, startAtLogin: false, transcriptRetentionDays: 0, microphoneName: null, onboardingComplete: true, onboardingVersion: "atmospeak-setup-v2" };
database.prepare("update settings set value=? where key='app'").run(JSON.stringify(settings));
// Keep realistic history/dictionary size without referring to installed recordings.
database.exec("update transcript_sessions set audio_path=''");
const priorRows = Number(database.prepare("select coalesce(max(rowid),0) as n from transcript_sessions").get().n);
const priorEvents = Number(database.prepare("select coalesce(max(id),0) as n from runtime_events").get().n);
const environment = { ...process.env, TEMP: join(runRoot, "tmp"), TMP: join(runRoot, "tmp"), ATMOSPEAK_APP_DATA_DIR: profile, WIND_SPEAK_APP_DATA_DIR: profile, ATMOSPEAK_NATIVE_HARNESS: "1", ATMOSPEAK_TEST_AUDIO_FIXTURE: recording, WEBVIEW2_USER_DATA_FOLDER: join(runRoot, "webview-profile"), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--disk-cache-dir=${join(runRoot, "webview-cache")}`, NODE_COMPILE_CACHE: join(runRoot, "node-cache") };
for (const key of ["TEMP", "WEBVIEW2_USER_DATA_FOLDER", "NODE_COMPILE_CACHE"]) mkdirSync(environment[key], { recursive: true });
delete environment.ATMOSPEAK_WEBVIEW_DEBUG_PORT;
const children = new Set();
function start(exe, arguments_, cwd = root) {
  const child = spawn(exe, arguments_, { cwd, env: environment, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  children.add(child);
  let output = "";
  child.stdout.on("data", (data) => { output += data; });
  child.stderr.on("data", (data) => { output += data; });
  child.output = () => output;
  return child;
}
async function exit(child, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  while (child.exitCode === null && child.signalCode === null && Date.now() < deadline) await sleep(10);
  if (child.exitCode !== 0) throw new Error(`Owned helper failed (${child.exitCode}): ${child.output().slice(-1500)}`);
}
async function until(action, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = action();
    if (result) return result;
    await sleep(10);
  }
  throw new Error(`${label} exceeded ${timeoutMs}ms`);
}
const jsonLines = (path) => existsSync(path) ? readFileSync(path, "utf8").split(/\r?\n/).filter(Boolean).flatMap((line) => { try { return [JSON.parse(line)]; } catch { return []; } }) : [];
const ps = ["-NoProfile", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-File"];
const rows = [];
let app;
try {
  await exit(start("powershell.exe", [...ps, join(root, "scripts/send-native-keys.ps1"), "-Action", "up", "-Keys", hotkey]));
  app = start(appExe, [], dirname(appExe));
  await until(() => {
    if (app.exitCode !== null) throw new Error(`Benchmark app exited: ${app.output().slice(-1000)}`);
    return database.prepare("select id from runtime_events where id>? and kind='streaming-asr-ready'").get(priorEvents);
  }, 25000, "CPU model ready");
  for (let iteration = 0; iteration < runs; iteration++) {
    const caseRoot = join(runRoot, `run-${iteration + 1}`);
    mkdirSync(caseRoot);
    const output = join(caseRoot, "native-text.txt");
    const ready = join(caseRoot, "native.ready");
    const editorEvents = join(caseRoot, "editor-events.jsonl");
    const keyEvents = join(caseRoot, "key-events.jsonl");
    const editor = start("powershell.exe", ["-STA", ...ps, join(root, "scripts/native-injection-target.ps1"), "-OutputPath", output, "-ReadyPath", ready, "-EventLogPath", editorEvents]);
    await until(() => existsSync(ready), 12000, "Owned native editor ready");
    const hwnd = readFileSync(ready, "utf8").trim();
    const cursor = Number(database.prepare("select coalesce(max(id),0) as n from runtime_events").get().n);
    const down = start("powershell.exe", [...ps, join(root, "scripts/send-native-keys.ps1"), "-Action", "down", "-Keys", hotkey, "-FocusHwnd", hwnd, "-EventLogPath", keyEvents]);
    await exit(down);
    const listening = await until(() => database.prepare("select created_at from runtime_events where id>? and kind='shortcut-ack' and message like '%->listening%' order by id desc limit 1").get(cursor), 8000, "Recording fixture started");
    // Start the stop controller immediately; its startup is outside the measured interval.
    const atUtc = Math.ceil(Date.parse(listening.created_at) + audioMs + 30);
    const up = start("powershell.exe", [...ps, join(root, "scripts/send-native-keys.ps1"), "-Action", "up", "-Keys", hotkey, "-AtUtcMs", String(atUtc), "-EventLogPath", keyEvents]);
    await exit(up, audioMs + 10000);
    const submit = jsonLines(keyEvents).find((event) => event.kind === "key-up");
    if (!submit) throw new Error("Missing native Submit edge");
    if (submit.timingVersion !== 2 || submit.inputEventCount !== hotkey.split("+").length) throw new Error("Missing native SendInput timing or incomplete chord");
    if (submit.utcMs - submit.requestedUtcMs > 100) throw new Error("Stop controller arrived late; do not report an artificially idle-tail result");
    const session = await until(() => database.prepare("select rowid,* from transcript_sessions where rowid>? and injected=1 order by rowid desc limit 1").get(priorRows + iteration), 30000, "Final text delivered");
    const expected = session.polished_text || session.cleaned_text;
    const visible = await until(() => {
      if (!existsSync(output)) return;
      const text = readFileSync(output, "utf8");
      if (text.replaceAll("\r\n", "\n") !== expected.replaceAll("\r\n", "\n")) return;
      const sha = createHash("sha256").update(text, "utf8").digest("hex");
      return jsonLines(editorEvents).find((event) => event.sha256 === sha && BigInt(event.ticks) >= BigInt(submit.ticks));
    }, 5000, "Exact text visible in owned editor");
    const metrics = await until(() => database.prepare("select * from dictation_metrics where session_id=?").get(session.id), 2000, "Streaming metrics saved");
    if (!metrics || (args["allow-fallback"] !== "true" && (metrics.audio_frames_dropped !== 0 || metrics.fallback_reason))) throw new Error("Streaming replay dropped audio or fell back");
    const captured = readFileSync(session.audio_path);
    if (captured.readUInt32LE(40) !== audioBytes) throw new Error("Captured fixture did not contain the entire original recording");
    const path = database.prepare("select message from runtime_events where kind='submit-path' and message like ? order by id desc limit 1").get(`%session=${session.id} %`);
    const row = { inputTimingVersion: 2, nativeInputCallMs: Number(BigInt(submit.inputCallReturnedTicks) - BigInt(submit.ticks)) * 1000 / Number(submit.frequency), iteration: iteration + 1, sessionId: session.id, audioMs, submitToVisibleMs: Number(BigInt(visible.ticks) - BigInt(submit.ticks)) * 1000 / Number(visible.frequency), visibleCharacters: visible.length, wordCount: session.word_count, textSha256: visible.sha256, capturedSamples: audioBytes / 2, streamingMetrics: metrics, submitPath: path?.message, exactReadback: true, stageLog: app.output().split(/\r?\n/).filter((line) => line.includes(`session=${session.id}`) && line.includes("stage-metrics")).at(-1) };
    rows.push(row);
    writeFileSync(join(runRoot, "results.json"), JSON.stringify({ label: args.label ?? "submit-to-native-text", executableSha256: createHash("sha256").update(readFileSync(appExe)).digest("hex"), recording, priorHistoryRows: priorRows, inputTimingVersion: 2, clock: "shared Windows QueryPerformanceCounter; immediately before native SendInput to editor TextChanged", audioMs, rows }, null, 2));
    console.log(JSON.stringify(row));
    editor.kill();
    children.delete(editor);
    await sleep(1300);
  }
} finally {
  for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill();
  writeFileSync(join(runRoot, "application.log"), app?.output() ?? "");
  database.close();
}
