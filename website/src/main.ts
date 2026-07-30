import "./styles.css";
import {
  APP_VERSION,
  FREE_CDN_BASE,
  NOVPAX_PRODUCT_URL,
  POLAR_CHECKOUT_URL,
} from "./version";

const releaseBaseUrl = FREE_CDN_BASE;
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
shell.append(
  hero(),
  featureBand(),
  modelSection(),
  downloadSection(),
  proSection(),
  docsSection(),
  footer(),
);
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
    navLink("Free download", "#downloads"),
    navLink("Pro", "#pro"),
    navLink("Docs", "#docs"),
    navLink("novpax.org", NOVPAX_PRODUCT_URL),
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
  title.append("Speak naturally. ");
  const accent = document.createElement("em");
  accent.textContent = "Paste once.";
  title.append(accent);
  content.append(title);

  const copy = document.createElement("p");
  copy.textContent =
    "Atmospeak streams transcription locally while you speak, previews it in the dock, and inserts one reconciled result when you stop.";
  content.append(copy);

  const actions = document.createElement("div");
  actions.className = "actions";
  const download = document.createElement("a");
  download.className = "button button-primary";
  download.href = releaseUrl(releaseArtifacts.setup);
  download.textContent = `Download free v${APP_VERSION}`;
  const buy = document.createElement("a");
  buy.className = "button";
  buy.href = POLAR_CHECKOUT_URL;
  buy.textContent = "Buy Pro — $69";
  buy.rel = "noreferrer";
  actions.append(download, buy, siteButton("Read install docs", "#docs"));
  content.append(actions);

  section.append(content);
  section.append(
    specStrip(`v${APP_VERSION}`, "Windows x64", "Free MIT · Pro gated separately"),
  );
  return section;
}

function featureBand() {
  const section = document.createElement("section");
  section.className = "feature-band";
  section.setAttribute("aria-label", "Product features");

  for (const item of [
    ["Stream locally", "Bounded Whisper segments are decoded while you record, leaving only the final tail to reconcile when you stop."],
    ["Offline by default", "Audio, previews, transcripts, settings, and speech models stay on your Windows PC. No account or cloud transcription is required."],
    ["Hold or Tap", "Release any required key in Hold mode, or press the chord a second time in Tap mode. Both finish with exactly one paste."],
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
    "Atmospeak keeps the selected GGML model warm in a crash-isolated streaming sidecar. Automatic mode tries Vulkan first, falls back to CPU, then retains the resident whisper-server and one-shot whisper-cli as local recovery paths.";
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
      name: "Distil Large v3 (legacy)",
      id: "distil-large-v3",
      use: "Existing installs · Settings",
      file: "ggml-distil-large-v3.bin",
      size: "1,450 MB",
      source: "distil-whisper",
      href: "https://huggingface.co/distil-whisper/distil-large-v3-ggml/resolve/main/ggml-distil-large-v3.bin",
    },
    {
      name: "Large v3 Turbo q5",
      id: "large-v3-turbo-q5",
      use: "Current multilingual · Settings",
      file: "ggml-large-v3-turbo-q5_0.bin",
      size: "548 MB",
      source: "ggerganov/whisper.cpp",
      href: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
    },
    {
      name: "Distil Large v3.5",
      id: "distil-large-v3.5",
      use: "Current English · Settings",
      file: "ggml-distil-large-v3.5.bin",
      size: "1,450 MB",
      source: "distil-whisper",
      href: "https://huggingface.co/distil-whisper/distil-large-v3.5-ggml/resolve/main/ggml-model.bin",
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
      "Automatic Balanced chooses only among models you installed and uses measured backlog and finalization time. It never downloads or deletes a model on its own.",
    ),
    modelNote(
      "Download integrity",
      "Every managed download is checked against a pinned SHA-256 hash before it is installed.",
    ),
    modelNote(
      "Advanced overrides",
      "You can pin a model, choose Vulkan or CPU, disable live preview, or turn off streaming with the documented environment switch.",
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
    "Free edition installers and updater metadata live at www.novpax.org/downloads/atmospeak/free/. Pro is a separate signed build sold through Polar — not these public links.";
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
      `SHA-256 hashes for every published v${APP_VERSION} release artifact.`,
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

function proSection() {
  const section = document.createElement("section");
  section.id = "pro";
  section.className = "docs";
  section.setAttribute("aria-labelledby", "pro-title");

  const header = document.createElement("div");
  header.className = "section-head";
  header.append(kicker("+ ATMOSPEAK PRO / SEPARATE BUILD"));
  const heading = document.createElement("h2");
  heading.id = "pro-title";
  heading.textContent = "Pro — $69, three years of gated updates";
  const copy = document.createElement("p");
  copy.textContent =
    "Atmospeak Pro is a separate binary with online Polar licensing, a private update channel, and Pro-only capabilities (airplane mode, network ledger; local meeting transcription planned). The free MIT dictation surface stays public forever. Atmos (meeting transcription) consolidates into Atmospeak Pro — not a second product.";
  header.append(heading, copy);
  section.append(header);

  const actions = document.createElement("div");
  actions.className = "actions";
  const buy = document.createElement("a");
  buy.className = "button button-primary";
  buy.href = POLAR_CHECKOUT_URL;
  buy.textContent = "Buy on Polar";
  buy.rel = "noreferrer";
  const canon = document.createElement("a");
  canon.className = "button";
  canon.href = NOVPAX_PRODUCT_URL;
  canon.textContent = "Canonical page on novpax.org";
  canon.rel = "noreferrer";
  actions.append(buy, canon);
  section.append(actions);
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
      "The default shortcut is Ctrl+Win in Hold mode.",
      "Hold the chord and release any required key to stop, or choose Tap and press the chord a second time.",
      "The dock previews locally while you speak; your target app receives one final paste.",
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
      "Atmospeak processes bounded speech segments during recording and freezes the timer immediately when you stop.",
      "Finalization time depends on the remaining tail, model size, Vulkan or CPU performance, and current backlog. Local diagnostics show the measured result.",
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
    "Free Atmospeak stays fully local. Canonical product and Pro checkout live on novpax.org; this GitHub Pages site is a transitional mirror. Pro is a separate licensed build with gated updates.";
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
