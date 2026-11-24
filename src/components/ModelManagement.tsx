import { Cpu } from "lucide-react";
import type {
  AppSettings,
  ModelDownloadProgress,
  ModelInventory,
} from "../types/dictation";
import { PanelTitle } from "./PanelTitle";

interface ModelManagementProps {
  settings: AppSettings;
  inventory: ModelInventory | null;
  download: ModelDownloadProgress | null;
  onSelect: (modelId: string) => void;
  onDownload: (modelId: string) => Promise<void>;
  onCancelDownload: () => Promise<void>;
  onDelete: (modelId: string) => Promise<void>;
}

const HANDY_BENCHMARK = "https://github.com/handy-computer/transcribe.cpp/blob/2778ae4bf5ca5f394d129a39467f4ad9552dc34f/catalog/parakeet-unified-en-0.6b.json";

function ParakeetDetails() {
  return (
    <>
      <small>English dictation with punctuation, capitalization, and live preview. Runs offline.</small>
      <dl className="model-benchmarks">
        <div><dt>English accuracy</dt><dd>A- (3.99% word error rate)</dd></div>
        <div><dt>Reference speed</dt><dd>2.15 s CPU, 1.19 s GPU per 30 s of audio</dd></div>
      </dl>
      <small>
        Handy reference benchmarks. Speed varies by hardware. This build uses the CPU.
      </small>
      <a href={HANDY_BENCHMARK} target="_blank" rel="noreferrer">Benchmark source</a>
    </>
  );
}

export function ModelManagement({
  settings,
  inventory,
  download,
  onSelect,
  onDownload,
  onCancelDownload,
  onDelete,
}: ModelManagementProps) {
  const selectedLabel =
    inventory?.models.find((model) => model.id === settings.activeModelId)?.label ??
    settings.activeModelId;
  return (
    <>
      <PanelTitle icon={<Cpu size={22} />} title="Voice models" />
      <p className="muted">
        Parakeet Unified English comes with Atmospeak and runs on your device.
      </p>
      <p className="muted">
        {selectedLabel} · CPU
      </p>
      <div className="model-grid">
        {inventory?.models.map((model) => {
          const active = settings.activeModelId === model.id && !settings.advancedRuntimeEnabled;
          const downloading =
            download?.modelId === model.id &&
            ["starting", "downloading", "verifying"].includes(download.status);
          return (
            <div key={model.id} className={`model-pill ${active ? "model-pill--active" : ""}`}>
              <strong>{model.label}</strong>
              <span>
                {model.sizeMb ? `${model.sizeMb} MiB · ` : ""}
                {!model.installed ? "not installed" : active ? "active" : model.bundled ? "bundled" : "installed"}
              </span>
              {model.id === "parakeet-unified-en" && <ParakeetDetails />}
              <div className="model-pill__actions">
                {downloading ? (
                  <button type="button" className="button button--ghost" onClick={() => void onCancelDownload()}>
                    Cancel {download?.percent != null ? `${Math.round(download.percent)}%` : ""}
                  </button>
                ) : model.installed ? (
                  <>
                    <button type="button" className="button button--ghost" onClick={() => onSelect(model.id)}>
                      {active ? "Selected" : "Use"}
                    </button>
                    {!model.bundled ? (
                      <button type="button" className="button button--ghost" onClick={() => void onDelete(model.id)}>
                        Delete
                      </button>
                    ) : null}
                  </>
                ) : (
                  <button
                    type="button"
                    className="button button--ghost"
                    disabled={Boolean(download && ["starting", "downloading", "verifying"].includes(download.status))}
                    onClick={() => void onDownload(model.id)}
                  >
                    Download
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
      {download?.message ? <p className="muted">{download.message}</p> : null}
    </>
  );
}
