import "./styles.css";

const releaseBaseUrl = "https://github.com/leviathofnoesia/wind-speak/releases/latest/download";

const downloads = [
  {
    label: "Windows installer",
    detail: "Recommended NSIS setup",
    file: "Wind-Speak_0.1.5_x64-setup.exe",
    primary: true,
  },
  {
    label: "Windows MSI",
    detail: "Enterprise-friendly fallback",
    file: "Wind-Speak_0.1.5_x64_en-US.msi",
    primary: false,
  },
  {
    label: "Portable zip",
    detail: "Unzip and run from a folder",
    file: "Wind-Speak_0.1.5_x64-portable.zip",
    primary: false,
  },
  {
    label: "Checksums",
    detail: "SHA-256 manifest",
    file: "SHA256SUMS.txt",
    primary: false,
  },
] as const;

function linkFor(file: string) {
  return `${releaseBaseUrl}/${file}`;
}

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("Missing app root");
}

const shell = document.createElement("main");
shell.className = "site-shell";

shell.appendChild(hero());
shell.appendChild(featureBand());
shell.appendChild(downloadSection());
shell.appendChild(footer());

app.appendChild(shell);

function hero() {
  const section = document.createElement("section");
  section.className = "plate hero";
  section.setAttribute("aria-labelledby", "hero-title");

  const nav = document.createElement("nav");
  nav.className = "top-nav";
  nav.setAttribute("aria-label", "Primary");
  nav.append(label("Wind Speak x Nov Pax", "brand-chip"));
  nav.append(navLink("Downloads", "#downloads"));
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
    "Wind Speak bundles its offline transcription engine, launches from your tray, and turns a global hotkey into clean pasted text.";
  content.append(copy);

  const actions = document.createElement("div");
  actions.className = "actions";
  actions.append(downloadButton("Download for Windows", downloads[0].file, true));
  actions.append(downloadButton("Portable zip", downloads[2].file, false));
  content.append(actions);

  section.append(content);
  section.append(specStrip("v0.1.5", "Windows x64", "Offline base.en model bundled"));
  return section;
}

function featureBand() {
  const section = document.createElement("section");
  section.className = "feature-band";
  section.setAttribute("aria-label", "Product features");

  for (const item of [
    ["Install once", "No model-picker detour. The first English model and whisper runtime are already inside the installer."],
    ["Offline by default", "Audio stays local. The app writes WAV, transcribes locally, cleans text, and injects through the clipboard."],
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

function downloadSection() {
  const section = document.createElement("section");
  section.id = "downloads";
  section.className = "downloads";
  section.setAttribute("aria-labelledby", "downloads-title");

  const header = document.createElement("div");
  header.className = "section-head";
  header.append(kicker("+ RELEASE FILES / GITHUB"));
  const heading = document.createElement("h2");
  heading.id = "downloads-title";
  heading.textContent = "Download Wind Speak";
  const copy = document.createElement("p");
  copy.textContent =
    "Installers, portable builds, updater metadata, and checksums are published as GitHub Release assets.";
  header.append(heading, copy);
  section.append(header);

  const grid = document.createElement("div");
  grid.className = "download-grid";
  for (const download of downloads) {
    const card = document.createElement("article");
    card.className = download.primary ? "download-card is-primary" : "download-card";
    card.append(kicker(download.primary ? "+ RECOMMENDED" : "+ ARTIFACT"));
    const title = document.createElement("h3");
    title.textContent = download.label;
    const detail = document.createElement("p");
    detail.textContent = download.detail;
    const anchor = document.createElement("a");
    anchor.href = linkFor(download.file);
    anchor.rel = "noopener";
    anchor.textContent = download.file;
    card.append(title, detail, anchor);
    grid.append(card);
  }
  section.append(grid);

  return section;
}

function footer() {
  const node = document.createElement("footer");
  node.className = "site-footer";
  node.append(kicker("+ TRUST MODEL"));
  const copy = document.createElement("p");
  copy.textContent =
    "Prototype builds are unsigned for Windows SmartScreen, but app updates are checked against Tauri updater signatures and release checksums.";
  node.append(copy);
  return node;
}

function downloadButton(text: string, file: string, primary: boolean) {
  const anchor = document.createElement("a");
  anchor.className = primary ? "button button-primary" : "button";
  anchor.href = linkFor(file);
  anchor.rel = "noopener";
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
