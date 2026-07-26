import "./styles.css";
import { APP_VERSION } from "./version";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("Missing app root");
}

const shell = document.createElement("main");
shell.className = "site-shell";
shell.append(hero(), featureBand(), downloadSection(), docsSection(), footer());
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
  navItems.append(navLink("Downloads", "#downloads"), navLink("Docs", "#docs"));
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
  const paused = document.createElement("a");
  paused.className = "button button-primary";
  paused.href = "#recovery-notice";
  paused.textContent = "Downloads paused";
  actions.append(
    paused,
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

function downloadSection() {
  const section = document.createElement("section");
  section.id = "downloads";
  section.className = "downloads";
  section.setAttribute("aria-labelledby", "downloads-title");

  const header = document.createElement("div");
  header.className = "section-head";
  header.append(kicker("+ RELEASE STATUS / RECOVERY"));
  const heading = document.createElement("h2");
  heading.id = "downloads-title";
  heading.textContent = "Downloads temporarily paused";
  const copy = document.createElement("p");
  copy.textContent =
    "Version 0.3.0 did not meet Atmospeak's first-run, microphone, performance, or interface standards. It has been withdrawn while 0.3.1 is validated.";
  header.append(heading, copy);
  section.append(header);

  const notice = document.createElement("article");
  notice.id = "recovery-notice";
  notice.className = "release-notice";
  notice.append(kicker("+ WHAT HAPPENS NEXT"));
  const title = document.createElement("h3");
  title.textContent = "0.3.1 is being tested before publication.";
  const detail = document.createElement("p");
  detail.textContent =
    "Downloads will return only after first-run setup, real-device dictation, dock interaction, installation, and a daily-driver test pass. No broken artifact is being offered in the meantime.";
  notice.append(title, detail);
  section.append(notice);
  return section;
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
