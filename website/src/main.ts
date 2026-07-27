import "./styles.css";
import { APP_VERSION } from "./version";

const releaseBaseUrl =
  "https://github.com/leviathofnoesia/atmospeak/releases/latest/download";
const releaseArtifacts = {
  setup: `atmospeak_${APP_VERSION}_x64-setup.exe`,
  msi: `atmospeak_${APP_VERSION}_x64_en-US.msi`,
  portable: `atmospeak_${APP_VERSION}_x64-portable.zip`,
  checksums: "SHA256SUMS.txt",
  updater: "latest.json",
} as const;

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("Missing app root");
}

const shell = document.createElement("main");
shell.className = "site-shell";
shell.append(hero(), featureBand(), modelSection(), downloadSection(), docsSection(), footer());
app.appendChild(shell);

function hero() {
  const section = document.createElement("section");
  section.className = "plate hero";
  section.setAttribute("aria-labelledby", "hero-title");

  const nav = document.createElement("nav");
  nav.className = "top-nav";
  nav.setAttribute("aria-label", "Primary");
  nav.append(label("Atmospeak × Nov Pax", "brand-chip"));
  const navItems = document.createElement("span");
  navItems.className = "nav-items";
  navItems.append(
    navLink("Models", "#models"),
    navLink("Downloads", "#downloads"),
    navLink("Docs", "#docs"),
  );
  nav.append(navItems);
  section.append(nav);

  const moon = document.createElement("div");
  moon.className = "halftone";
  moon.setAttribute("aria-hidden", "true");
  section.append(moon);

  const content = document.createElement("div");
  content.className = "hero-content";
  content.append(kicker("+ DESKTOP DICTATION / LOCAL RUNTIME"));

  const title = document.createElement("h1");
  title.id = "hero-title";
  title.append("Speak anywhere. ");
  const accent = document.createElement("em");
  accent.textContent = "Paste instantly.";
  title.append(accent);
  content.append(title);

  const copy = document.createElement("p");
  copy.textContent =
    "Atmospeak bundles an offline transcription engine, launches from your tray, and turns a global hotkey into clean pasted text.";
  content.append(copy);

  const actions = document.createElement("div");
  actions.className = "actions";
  const download = document.createElement("a");
  download.className = "button button-primary";
  download.href = releaseUrl(releaseArtifacts.setup);
  download.textContent = `Download v${APP_VERSION}`;
  actions.append(
    download,
    siteButton("Read install docs", "#docs"),
  );
  content.append(actions);

  section.append(content);
  section.append(specStrip(`v${APP_VERSION}`, "Windows x64", "Offline base.en model bundled"));
  return section;
}

function featureBand() {
  const section = document.createElement("section");
  section.className = "feature-band";
  section.setAttribute("aria-label", "Product features");

  for (const item of [
    ["Install once", "The first English model and whisper runtime are already inside the installer. Optional models download in the app."],
    ["Offline by default", "Audio, transcripts, settings, and speech models stay on your Windows PC. No account or cloud transcription is required."],
    ["Speak anywhere", "Global push-to-talk works across Notepad, editors, browsers, chat fields, and terminals."],
  ] as const) {
    const card = document.createElement("article");
    card.className = "feature-card";
    card.append(kicker(`+ ${item[0]}`));
    const heading = document.createElement("h2");
    heading.textContent = item[0];
    const copy = document.createElement("p");
    copy.textContent = item[1];
    card.append(heading, copy);
    section.append(card);
  }

  return section;
}

function modelSection() {
  const section = document.createElement("section");
  section.id = "models";
  section.className = "models";
  section.setAttribute("aria-labelledby", "models-title");

  const header = document.createElement("div");
  header.className = "section-head model-head";
  header.append(kicker("+ UNDER THE HOOD / EXACT MODELS"));
  const heading = document.createElement("h2");
  heading.id = "models-title";
  heading.textContent = "Local Whisper models";
  const copy = document.createElement("p");
  copy.textContent =
    "Atmospeak runs the Windows x64 whisper.cpp runtime locally. It prefers a resident whisper-server that keeps the selected GGML model warm, then falls back to the bundled one-shot whisper-cli if that host is unavailable.";
  header.append(heading, copy);
  section.append(header);

  const table = document.createElement("div");
  table.className = "model-table";
  table.setAttribute("role", "table");
  table.setAttribute("aria-label", "Atmospeak speech recognition models");

  const tableHeader = document.createElement("div");
  tableHeader.className = "model-row model-row--header";
  tableHeader.setAttribute("role", "row");
  for (const text of ["Model", "Used in Atmospeak", "File", "Size", "Source"]) {
    const cell = document.createElement("span");
    cell.setAttribute("role", "columnheader");
    cell.textContent = text;
    tableHeader.append(cell);
  }
  table.append(tableHeader);

  const models = [
    {
      name: "Tiny English",
      id: "tiny.en",
      use: "Swift · setup option",
      file: "ggml-tiny.en.bin",
      size: "75 MB",
      source: "ggerganov/whisper.cpp",
      href: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
    },
    {
      name: "Base English",
      id: "base.en",
      use: "Balanced · bundled default",
      file: "ggml-base.en.bin",
      size: "142 MB",
      source: "ggerganov/whisper.cpp",
      href: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
    },
    {
      name: "Small English",
      id: "small.en",
      use: "Faithful · setup option",
      file: "ggml-small.en.bin",
      size: "466 MB",
      source: "ggerganov/whisper.cpp",
      href: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
    },
    {
      name: "Medium English",
      id: "medium.en",
      use: "Optional · Settings",
      file: "ggml-medium.en.bin",
      size: "1,463 MB",
      source: "ggerganov/whisper.cpp",
      href: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin",
    },
    {
      name: "Distil Large v3",
      id: "distil-large-v3",
      use: "Optional · Settings",
      file: "ggml-distil-large-v3.bin",
      size: "1,450 MB",
      source: "distil-whisper",
      href: "https://huggingface.co/distil-whisper/distil-large-v3-ggml/resolve/main/ggml-distil-large-v3.bin",
    },
  ] as const;

  for (const model of models) {
    const row = document.createElement("div");
    row.className = model.id === "base.en" ? "model-row model-row--bundled" : "model-row";
    row.setAttribute("role", "row");

    const identity = document.createElement("span");
    identity.className = "model-identity";
    identity.setAttribute("role", "cell");
    const name = document.createElement("strong");
    name.textContent = model.name;
    const id = document.createElement("code");
    id.textContent = model.id;
    identity.append(name, id);

    const use = document.createElement("span");
    use.setAttribute("role", "cell");
    use.textContent = model.use;

    const file = document.createElement("code");
    file.setAttribute("role", "cell");
    file.textContent = model.file;

    const size = document.createElement("span");
    size.setAttribute("role", "cell");
    size.textContent = model.size;

    const source = document.createElement("a");
    source.setAttribute("role", "cell");
    source.href = model.href;
    source.textContent = model.source;
    source.rel = "noreferrer";

    row.append(identity, use, file, size, source);
    table.append(row);
  }
  section.append(table);

  const notes = document.createElement("div");
  notes.className = "model-notes";
  notes.append(
    modelNote(
      "Selection safety",
      "If a selected optional model is missing, Atmospeak automatically uses the bundled base.en model instead.",
    ),
    modelNote(
      "Download integrity",
      "Every managed download is checked against a pinned SHA-256 hash before it is installed.",
    ),
    modelNote(
      "Advanced overrides",
      "Settings can point to a custom whisper.cpp CLI and model path; this bypasses the managed list above.",
    ),
  );
  section.append(notes);
  return section;
}

function modelNote(titleText: string, description: string) {
  const note = document.createElement("article");
  const title = document.createElement("h3");
  title.textContent = titleText;
  const copy = document.createElement("p");
  copy.textContent = description;
  note.append(title, copy);
  return note;
}

function downloadSection() {
  const section = document.createElement("section");
  section.id = "downloads";
  section.className = "downloads";
  section.setAttribute("aria-labelledby", "downloads-title");

  const header = document.createElement("div");
  header.className = "section-head";
  header.append(kicker("+ RELEASE / WINDOWS X64"));
  const heading = document.createElement("h2");
  heading.id = "downloads-title";
  heading.textContent = `Atmospeak v${APP_VERSION}`;
  const copy = document.createElement("p");
  copy.textContent =
    "Install the corrective Windows release, use the portable build, or verify every file against the published checksums.";
  header.append(heading, copy);
  section.append(header);

  const grid = document.createElement("div");
  grid.className = "download-grid";
  grid.append(
    downloadCard(
      "Setup EXE",
      "Recommended",
      "Standard Windows installer with the bundled offline English model.",
      releaseArtifacts.setup,
      true,
    ),
    downloadCard(
      "MSI",
      "Managed install",
      "Windows Installer package for users who prefer MSI deployment.",
      releaseArtifacts.msi,
    ),
    downloadCard(
      "Portable ZIP",
      "No installer",
      "Extract and run Atmospeak without installing it system-wide.",
      releaseArtifacts.portable,
    ),
    downloadCard(
      "Checksums",
      "Verify files",
      "SHA-256 hashes for every published v0.3.1 release artifact.",
      releaseArtifacts.checksums,
    ),
    downloadCard(
      "Updater metadata",
      "latest.json",
      "Signed metadata used by Atmospeak's built-in update checker.",
      releaseArtifacts.updater,
    ),
  );
  section.append(grid);
  return section;
}

function downloadCard(
  titleText: string,
  labelText: string,
  description: string,
  artifact: string,
  primary = false,
) {
  const card = document.createElement("article");
  card.className = primary ? "download-card is-primary" : "download-card";
  card.append(kicker(`+ ${labelText}`));
  const title = document.createElement("h3");
  title.textContent = titleText;
  const copy = document.createElement("p");
  copy.textContent = description;
  const link = document.createElement("a");
  link.href = releaseUrl(artifact);
  link.textContent = `Download ${artifact}`;
  card.append(title, copy, link);
  return card;
}

function releaseUrl(artifact: string) {
  return `${releaseBaseUrl}/${artifact}`;
}

function docsSection() {
  const section = document.createElement("section");
  section.id = "docs";
  section.className = "docs";
  section.setAttribute("aria-labelledby", "docs-title");

  const header = document.createElement("div");
  header.className = "section-head";
  header.append(kicker("+ GET STARTED / WINDOWS"));
  const heading = document.createElement("h2");
  heading.id = "docs-title";
  heading.textContent = "Install and dictate";
  const copy = document.createElement("p");
  copy.textContent =
    "Atmospeak is currently an unsigned Windows build. These steps cover the warning, first-run setup, daily hotkey use, and local data.";
  header.append(heading, copy);
  section.append(header);

  const grid = document.createElement("div");
  grid.className = "docs-grid";
  grid.append(
    docCard("01 / Install", [
      "Download the recommended setup executable above.",
      "Open it and complete the installer.",
      "Atmospeak starts in the tray and opens setup only on first run.",
    ]),
    docCard("02 / SmartScreen", [
      "Windows may show “Windows protected your PC” because this release is not Authenticode-signed.",
      "Choose More info, verify the app name is Atmospeak, then choose Run anyway.",
      "You can verify the downloaded file against SHA256SUMS.txt before running it.",
    ], true),
    docCard("03 / Hotkey", [
      "The default shortcut is Ctrl+Win in push-to-talk mode.",
      "Hold the chord, speak, then release to transcribe and paste at your cursor.",
      "Change the shortcut or switch to tap-to-toggle in Settings.",
    ]),
    docCard("04 / Microphone", [
      "Choose an input during onboarding and complete the level check.",
      "If no signal appears, open Windows Settings → Privacy & security → Microphone and allow desktop apps.",
      "Return to Atmospeak and run onboarding again from Settings.",
    ]),
    docCard("05 / Local data", [
      "Settings, history, recordings, and downloaded models live under %LOCALAPPDATA%\\Atmospeak.",
      "Transcription runs on-device. Removing that folder resets the local profile after the app is closed.",
    ]),
    docCard("06 / Latency", [
      "Latency depends on speech length, model size, CPU, and whether the resident model has finished warming.",
      "Short phrases can land quickly after warm-up; long dictation and larger models take longer. No fixed latency is promised.",
    ]),
  );
  section.append(grid);
  return section;
}

function docCard(titleText: string, steps: readonly string[], wide = false) {
  const article = document.createElement("article");
  article.className = wide ? "doc-card doc-card--wide" : "doc-card";
  const title = document.createElement("h3");
  title.textContent = titleText;
  const list = document.createElement("ol");
  for (const step of steps) {
    const item = document.createElement("li");
    item.textContent = step;
    list.append(item);
  }
  article.append(title, list);
  return article;
}

function footer() {
  const node = document.createElement("footer");
  node.className = "site-footer";
  node.append(kicker("+ TRUST MODEL"));
  const copy = document.createElement("p");
  copy.textContent =
    "Windows builds are unsigned, so SmartScreen can warn. Verify release checksums; in-app updates additionally require Tauri updater signatures when published.";
  node.append(copy);
  return node;
}

function siteButton(text: string, href: string) {
  const anchor = document.createElement("a");
  anchor.className = "button";
  anchor.href = href;
  anchor.textContent = text;
  return anchor;
}

function navLink(text: string, href: string) {
  const anchor = document.createElement("a");
  anchor.href = href;
  anchor.textContent = text;
  return anchor;
}

function kicker(text: string) {
  return label(text, "kicker");
}

function label(text: string, className: string) {
  const span = document.createElement("span");
  span.className = className;
  span.textContent = text;
  return span;
}

function specStrip(version: string, platform: string, runtime: string) {
  const strip = document.createElement("div");
  strip.className = "spec-strip";
  for (const item of [version, platform, runtime]) {
    const span = document.createElement("span");
    span.textContent = item;
    strip.append(span);
  }
  return strip;
}
