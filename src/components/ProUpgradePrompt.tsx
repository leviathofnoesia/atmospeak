import { Lock, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { ATMOSPEAK_PRO_BUY_URL, ATMOSPEAK_PRO_INFO_URL, openExternalUrl } from "../lib/api";

export interface ProUpgradePromptProps {
  variant: "workspace" | "privacy";
}

const PROMPT_COPY = {
  workspace: {
    kicker: "Atmospeak Pro · Optional upgrade",
    title: "Private Workspace archives, Airplane Mode & Network Ledger",
    body:
      "Free keeps unlimited local dictation and History search unlocked forever. Upgrade to Pro " +
      "for $69 once via Polar (3 years of updates on up to 3 Windows PCs) for bulk Markdown and " +
      "JSON archive exports, fail-closed Airplane Mode, and an inspectable Network Ledger.",
    regionLabel: "Atmospeak Pro workspace upgrade",
  },
  privacy: {
    kicker: "Atmospeak Pro · Verifiable offline guard",
    title: "Unlock fail-closed Airplane Mode & Network Ledger",
    body:
      "Atmospeak Pro blocks outbound update checks, account refreshes, and model downloads " +
      "before they start and logs every attempt to network_ledger.jsonl. $69 one-time via Polar (3 years of " +
      "updates, up to 3 devices).",
    regionLabel: "Atmospeak Pro privacy upgrade",
  },
} as const;

export function ProUpgradePrompt({ variant }: ProUpgradePromptProps) {
  const copy = PROMPT_COPY[variant];
  const [openError, setOpenError] = useState<string | null>(null);
  const Icon = variant === "workspace" ? Lock : ShieldCheck;

  const launchUrl = async (url: string) => {
    setOpenError(null);
    try {
      await openExternalUrl(url);
    } catch {
      setOpenError(`Open ${url} in your browser to continue.`);
    }
  };

  return (
    <section
      className="pro-upgrade-card"
      role="region"
      aria-label={copy.regionLabel}
      data-testid={`pro-upgrade-${variant}`}
    >
      <div className="pro-upgrade-card__head">
        <span className="pro-upgrade-card__badge">
          <Icon size={14} aria-hidden="true" />
          {copy.kicker}
        </span>
        <span className="pro-upgrade-card__price">$69 once · Polar</span>
      </div>
      <h3>{copy.title}</h3>
      <p>{copy.body}</p>
      <div className="pro-upgrade-card__actions">
        <button
          type="button"
          className="pill-btn primary"
          onClick={() => void launchUrl(ATMOSPEAK_PRO_BUY_URL)}
        >
          Upgrade to Pro — $69 once
        </button>
        <button
          type="button"
          className="pill-btn ghost"
          onClick={() => void launchUrl(ATMOSPEAK_PRO_INFO_URL)}
        >
          Compare Free vs Pro
        </button>
      </div>
      {openError ? (
        <p className="pro-upgrade-card__fallback" role="status">
          {openError}
        </p>
      ) : null}
    </section>
  );
}
