import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import {
  accuracyGrade,
  BENCHMARK_DICTATION_SECONDS,
  canonicalLanguage,
  compareEngineVersions,
  languageIdentity,
  modelFile,
} from "@handy-computer/models";

const require = createRequire(import.meta.url);
const source = require("@handy-computer/models/catalog.json");
const manifest = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
const vendorDirectory = dirname(require.resolve("@handy-computer/models/catalog.json"));
const vendor = JSON.parse(await readFile(join(vendorDirectory, "package.json"), "utf8"));
if (vendor.version !== manifest.devDependencies["@handy-computer/models"]) {
  throw new Error("Install the pinned Handy package before generating the catalog");
}
const engineVersion = "0.4.1";
const engineFamilies = new Set([
  "parakeet", "cohere_asr", "canary", "qwen3_asr", "moss", "voxtral", "voxtral_realtime",
  "canary_qwen", "whisper", "moonshine", "moonshine_streaming", "sensevoice",
  "funasr_nano", "gigaam", "granite_speech", "granite_speech_nar", "granite5_ctc", "medasr",
]);
const recommended = new Set([
  "parakeet-unified-en-0.6b", "nemotron-3.5-asr-streaming-0.6b", "canary-180m-flash",
  "parakeet-tdt-0.6b-v3", "whisper-large-v3-turbo", "qwen3-asr-0.6b",
]);
const communityLicensedMoonshine = /^moonshine-(?:(?:base|tiny)-(?:ar|ja|ko|uk|vi|zh)|base-es)$/;
// Keep existing settings IDs and already-installed Nemotron weights usable.
const variants = new Map([
  ["parakeet-unified-en-0.6b", { id: "parakeet-unified-en" }],
  ["nemotron-speech-streaming-en-0.6b", { id: "nemotron-streaming-en", quant: "Q4_K_M" }],
  ["nemotron-3.5-asr-streaming-0.6b", { id: "nemotron-streaming-3.5", quant: "Q4_K_M" }],
]);

function benchmark(model, languages) {
  const result = source.results[model.id];
  if (!result) return null;
  const accuracy = Object.fromEntries(languages.flatMap((language) => {
    const cell = result.accuracy[languageIdentity(language)];
    return cell ? [[language, { grade: accuracyGrade(cell.error).label, errorPercent: cell.error }]] : [];
  }));
  const cpuFactor = result.performance.cpu;
  const cpuSeconds = cpuFactor ? BENCHMARK_DICTATION_SECONDS / cpuFactor : null;
  return { accuracy, cpuSeconds };
}

if (source.schema !== 2) throw new Error("Unsupported Handy catalog schema");
const ids = new Set();
const files = new Set();
const models = source.models.flatMap((model) => {
  if (model.role !== "asr" || !engineFamilies.has(model.family) ||
      /(?:^|[^a-z0-9])nc(?:$|[^a-z0-9])/i.test(model.license) ||
      communityLicensedMoonshine.test(model.id) ||
      compareEngineVersions(model.minEngine ?? source.transcribeCpp, engineVersion) > 0) {
    return [];
  }

  const variant = variants.get(model.id);
  const id = variant?.id ?? model.id;
  const file = variant?.quant ? model.files[variant.quant] : modelFile(model);
  const languages = [...new Set(model.languages.map(canonicalLanguage))];
  if (!file || !/^[\w.-]+\.gguf$/i.test(file.filename) || file.filename.startsWith(".") ||
      !/^\w[\w.-]*\/\w[\w.-]*$/.test(model.repository) || !/^[0-9a-f]{40}$/.test(model.revision) ||
      !/^[0-9a-f]{64}$/.test(file.sha256) || !Number.isSafeInteger(file.size) || file.size <= 0 ||
      languages.length === 0 || ids.has(id) || files.has(file.filename.toLowerCase())) {
    throw new Error(`Invalid or duplicate speech model metadata for ${model.id}`);
  }
  ids.add(id);
  files.add(file.filename.toLowerCase());

  const sourceUrl = `https://huggingface.co/${model.repository}`;
  return [{
    id,
    label: model.name,
    filename: file.filename,
    bytes: file.size,
    sha256: file.sha256,
    url: `${sourceUrl}/resolve/${model.revision}/${file.filename}`,
    details: {
      description: model.description,
      languages,
      supportsStreaming: model.capabilities.streaming,
      supportsLanguageDetection: model.capabilities.languageDetection,
      recommended: recommended.has(model.id),
      sourceUrl,
      benchmark: variant?.quant ? null : benchmark(model, languages),
    },
  }];
});

const defaultId = "parakeet-unified-en";
const defaultModel = models.find((model) => model.id === defaultId);
if (!defaultModel?.details.supportsStreaming || !defaultModel.details.languages.includes("en")) {
  throw new Error("The default English Parakeet model is missing from the catalog");
}

const target = new URL("../src-asr-protocol/src/speech_catalog.json", import.meta.url);
const output = JSON.stringify({
  source: "@handy-computer/models",
  sourceVersion: vendor.version,
  engineVersion,
  models,
}, null, 2) + "\n";

if (process.argv.includes("--check")) {
  const existing = (await readFile(target, "utf8")).replaceAll("\r\n", "\n");
  if (existing !== output) throw new Error("Run bun run speech:sync-catalog to regenerate the catalog");
} else {
  await writeFile(target, output);
}
