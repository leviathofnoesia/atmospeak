import { readFileSync } from "node:fs";
import { renderToString } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App, { markOverlayDocument } from "./App";

const appStyles = readFileSync("src/App.css", "utf8");

describe("boot shell before effects run", () => {
  let style: HTMLStyleElement;
  let root: HTMLDivElement;

  beforeEach(() => {
    style = document.createElement("style");
    style.textContent = appStyles;
    document.head.append(style);
    root = document.createElement("div");
    document.body.append(root);
  });

  afterEach(() => {
    root.remove();
    style.remove();
    document.documentElement.classList.remove("is-overlay-window");
    document.body.classList.remove("is-overlay-window");
  });

  function renderBeforeEffects() {
    // Server rendering exposes the initial render without flushing useEffect,
    // unlike testing-library's render, which would already select OverlayShell.
    root.innerHTML = renderToString(<App />);
    return root.querySelector<HTMLElement>(".boot-shell")!;
  }

  it("retains the main window's centered, opaque loading screen", () => {
    const boot = renderBeforeEffects();
    expect(boot).not.toBeNull();
    const styles = getComputedStyle(boot);
    expect(styles.display).toBe("grid");
    expect(styles.background).toContain("radial-gradient");
    expect(styles.background).toContain("var(--paper)");
    expect(getComputedStyle(boot.querySelector("p")!).textTransform).toBe("uppercase");
  });

  it("does not paint the main window's boot background in the transparent overlay", () => {
    // Match main.tsx's pre-render setup for index.html?view=overlay.
    markOverlayDocument();
    const boot = renderBeforeEffects();
    expect(boot).not.toBeNull();
    const styles = getComputedStyle(boot);
    expect(styles.backgroundColor).toBe("rgba(0, 0, 0, 0)");
    expect(styles.backgroundImage).toBe("none");
    expect(styles.background).not.toContain("radial-gradient");
    expect(styles.background).not.toContain("var(--paper)");
    expect(getComputedStyle(document.documentElement).backgroundColor).toBe("rgba(0, 0, 0, 0)");
    expect(getComputedStyle(document.body).backgroundColor).toBe("rgba(0, 0, 0, 0)");
  });
});
