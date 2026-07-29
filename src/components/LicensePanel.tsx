import { WifiOff } from "lucide-react";
import { useEffect, useState } from "react";
import { activateLicense, deactivateLicense, getLicenseStatus } from "../lib/api";
import type { LicenseFeature, LicenseStatus } from "../types/dictation";
import { freeLicenseStatus } from "../types/dictation";

/**
 * Labels for the capabilities a licence unlocks. Every entry here is
 * functionality that did not exist in 0.5.3 — nothing that already shipped for
 * free is listed, and nothing that already shipped may be added later.
 */
const FEATURE_LABELS: Record<LicenseFeature, string> = {
  compliancePack: "Compliance pack — attested airplane mode, network ledger, audit export",
  voiceMacros: "Voice-to-action macros",
  mcpServer: "MCP server for coding agents",
  ideAwareness: "Editor symbol awareness and per-app profiles",
  sync: "Encrypted cross-device sync",
  teamSharedVocabulary: "Shared team vocabulary",
};

const TIER_LABELS: Record<LicenseStatus["tier"], string> = {
  free: "Free",
  pro: "Pro",
  team: "Team",
};

export function LicensePanel() {
  const [status, setStatus] = useState<LicenseStatus>(freeLicenseStatus());
  const [keyDraft, setKeyDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    void getLicenseStatus()
      .then(setStatus)
      .catch(() => setStatus(freeLicenseStatus()));
  }, []);

  const onActivate = async () => {
    if (!keyDraft.trim()) return;
    setBusy(true);
    setMessage("");
    setFailed(false);
    try {
      const next = await activateLicense(keyDraft.trim());
      setStatus(next);
      setKeyDraft("");
      setMessage(`${TIER_LABELS[next.tier]} licence activated.`);
    } catch (error: unknown) {
      setFailed(true);
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const onDeactivate = async () => {
    setBusy(true);
    setMessage("");
    setFailed(false);
    try {
      setStatus(await deactivateLicense());
      setMessage("Licence removed from this machine. The key itself stays valid.");
    } catch (error: unknown) {
      setFailed(true);
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <div className="hub__head">
        <div className="kick">P.05 / Licence — pay once, keep it</div>
        <h1>Your <em>licence.</em></h1>
      </div>
      <section className="settings-panel">
        <p className="muted">
          Dictation, local models, cleanup, Backtrack, history, dictionary, snippets, and on-device
          AI edit are free, unlimited, and always will be. A licence adds capability on top; it
          never takes anything away.
        </p>

        <div className="license-status">
          <p>
            <strong>{TIER_LABELS[status.tier]}</strong>
            {status.activated ? null : " — no licence on this machine"}
          </p>
          {status.activated ? (
            <>
              <p className="muted">Licence {status.licenseId}</p>
              <p className="muted">
                {status.inUpdateWindow
                  ? `Includes updates through ${status.updatesUntil}.`
                  : `Update window ended ${status.updatesUntil}. This build (released ${status.buildReleasedOn}) is newer, so paid features are inactive here. Reinstalling a build from within your window restores them.`}
              </p>
              {status.seats > 1 ? <p className="muted">{status.seats} seats</p> : null}
            </>
          ) : null}
        </div>

        {status.features.length > 0 ? (
          <ul className="license-features">
            {status.features.map((feature) => (
              <li key={feature}>{FEATURE_LABELS[feature]}</li>
            ))}
          </ul>
        ) : null}

        {status.activated ? (
          <button
            type="button"
            className="button button--ghost"
            disabled={busy}
            onClick={() => void onDeactivate()}
          >
            Remove licence from this machine
          </button>
        ) : (
          <div className="license-activate">
            <label>
              <span>Licence key</span>
              <textarea
                rows={3}
                spellCheck={false}
                placeholder="ATMO-..."
                value={keyDraft}
                onChange={(event) => setKeyDraft(event.currentTarget.value)}
              />
            </label>
            <button
              type="button"
              className="button"
              disabled={busy || !keyDraft.trim()}
              onClick={() => void onActivate()}
            >
              Activate
            </button>
          </div>
        )}

        {message ? (
          <p className={failed ? "license-message is-error" : "license-message"} role="status">
            {message}
          </p>
        ) : null}

        <p className="muted license-offline-note">
          <WifiOff size={14} /> Activation happens entirely on this machine. Atmospeak contacts no
          server to check your licence — not now, and not later. You can activate with the network
          disconnected.
        </p>
      </section>
    </div>
  );
}
