import { useCallback, useEffect, useRef, useState } from "react";
import {
  activateLicense,
  checkProUpdate,
  deactivateLicense,
  exportNetworkLedger,
  getLicenseStatus,
  getProFeatureStatus,
  setAirplaneMode,
  validateLicense,
  type LicenceStatus,
  type ProFeatureStatus,
} from "../lib/api";

interface ProPanelProps {
  onNotice: (tone: "success" | "error" | "neutral", message: string) => void;
}

export function ProPanel({ onNotice }: ProPanelProps) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [licence, setLicence] = useState<LicenceStatus | null>(null);
  const [features, setFeatures] = useState<ProFeatureStatus | null>(null);
  const onNoticeRef = useRef(onNotice);
  onNoticeRef.current = onNotice;

  const refresh = useCallback(async () => {
    const status = await getLicenseStatus();
    setLicence(status);
    if (status.valid) {
      try {
        setFeatures(await getProFeatureStatus());
      } catch {
        setFeatures(null);
      }
    } else {
      setFeatures(null);
    }
  }, []);

  useEffect(() => {
    void refresh().catch((error) => {
      onNoticeRef.current(
        "error",
        error instanceof Error ? error.message : String(error),
      );
    });
  }, [refresh]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
      await refresh();
    } catch (error) {
      onNotice("error", error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel-stack" aria-labelledby="pro-title">
      <header className="panel-header">
        <div className="kick">P.06 / Pro</div>
        <h2 id="pro-title">Atmospeak Pro</h2>
        <p className="panel-lede">
          Online Polar licence, gated updates, airplane mode, and network ledger.
          Free dictation stays unlimited — Pro is a separate build and policy.
        </p>
      </header>

      <div className="settings-card">
        <h3>Licence</h3>
        <p className="muted">
          {licence?.message ?? "Loading licence status…"}
        </p>
        {licence?.licenseDisplay ? (
          <p>
            Key <code>{licence.licenseDisplay}</code>
            {licence.updatesUntil ? (
              <>
                {" "}
                · updates until <code>{licence.updatesUntil}</code>
              </>
            ) : null}
            {licence.offlineGrace ? " · offline grace active" : null}
          </p>
        ) : null}
        <label className="field">
          <span>Polar licence key</span>
          <input
            value={key}
            onChange={(event) => setKey(event.target.value)}
            placeholder="Paste key from Polar customer portal"
            autoComplete="off"
            disabled={busy}
          />
        </label>
        <div className="button-row">
          <button
            type="button"
            className="button button--primary"
            disabled={busy || !key.trim()}
            onClick={() =>
              void run(async () => {
                const next = await activateLicense(key.trim());
                setKey("");
                onNotice("success", next.message);
              })
            }
          >
            Activate
          </button>
          <button
            type="button"
            className="button"
            disabled={busy || !licence?.activated}
            onClick={() =>
              void run(async () => {
                const next = await validateLicense();
                onNotice("success", next.message);
              })
            }
          >
            Validate online
          </button>
          <button
            type="button"
            className="button"
            disabled={busy || !licence?.activated}
            onClick={() =>
              void run(async () => {
                const next = await deactivateLicense();
                onNotice("neutral", next.message);
              })
            }
          >
            Remove licence
          </button>
          <button
            type="button"
            className="button"
            disabled={busy || !licence?.valid}
            onClick={() =>
              void run(async () => {
                const update = await checkProUpdate();
                onNotice(
                  "neutral",
                  update.available
                    ? `Pro update available: ${update.version}`
                    : "Pro is up to date (or no gated update published).",
                );
              })
            }
          >
            Check Pro updates
          </button>
        </div>
      </div>

      <div className="settings-card">
        <h3>Compliance pack</h3>
        <p className="muted">
          Airplane mode blocks new outbound model downloads and update checks.
          The network ledger records outbound attempts for export.
        </p>
        <div className="button-row">
          <button
            type="button"
            className="button"
            disabled={busy || !licence?.valid}
            onClick={() =>
              void run(async () => {
                const enabled = !features?.airplaneMode.enabled;
                const next = await setAirplaneMode(enabled);
                onNotice(
                  "success",
                  next.enabled ? "Airplane mode on." : "Airplane mode off.",
                );
              })
            }
          >
            {features?.airplaneMode.enabled
              ? "Disable airplane mode"
              : "Enable airplane mode"}
          </button>
          <button
            type="button"
            className="button"
            disabled={busy || !licence?.valid}
            onClick={() =>
              void run(async () => {
                const jsonl = await exportNetworkLedger();
                await navigator.clipboard.writeText(jsonl);
                onNotice("success", "Network ledger copied as JSONL.");
              })
            }
          >
            Export ledger
          </button>
        </div>
        {features?.ledgerRecent?.length ? (
          <ul className="pro-ledger">
            {features.ledgerRecent
              .slice()
              .reverse()
              .slice(0, 12)
              .map((entry) => (
                <li key={`${entry.at}-${entry.kind}-${entry.target}`}>
                  <code>{entry.kind}</code> → {entry.target}{" "}
                  {entry.allowed ? "allowed" : "blocked"}
                </li>
              ))}
          </ul>
        ) : (
          <p className="muted">No ledger entries yet.</p>
        )}
      </div>
    </section>
  );
}
