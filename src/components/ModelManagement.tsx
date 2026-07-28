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

const MODEL_DETAILS: Record<string, string> = {
  "tiny.en": "Fastest English option for short drafts.",
  "base.en": "Bundled English default with the smallest no-download setup.",
  "small.en": "More accurate English model for names, accents, and jargon.",
  "medium.en": "Accuracy-first English model with a larger memory footprint.",
  "distil-large-v3": "Previous-generation distilled English model; retained for existing installs.",
  "large-v3-turbo-q5": "New: multilingual Large v3 Turbo, quantized to about 548 MB.",
  "distil-large-v3.5": "New: latest distilled English model with stronger short-form robustness.",
};

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
  const accelerationLabel =
    settings.accelerationPreference === "auto"
      ? "Vulkan with CPU fallback"
      : settings.accelerationPreference === "vulkan"
        ? "Vulkan"
        : "CPU";
  return (
    <>
      <PanelTitle icon={<Cpu size={22} />} title="Voice models" />
      <p className="muted">
        Balanced is bundled. Large v3 Turbo q5 and Distil Large v3.5 are the newest optional
        choices; every download stays on this device and is SHA-256 verified.
      </p>
      <p className="muted">
        {settings.modelSelectionMode === "automatic" ? "Automatic" : "Manual"} ·{" "}
        {selectedLabel} · {accelerationLabel}
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
                {model.sizeMb ? `${model.sizeMb} MB · ` : ""}
                {!model.installed ? "not installed" : active ? "active" : model.bundled ? "bundled" : "installed"}
              </span>
              <small>{MODEL_DETAILS[model.id] ?? "Local whisper.cpp speech recognition model."}</small>
              {!model.bundled ? (
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
                      <button type="button" className="button button--ghost" onClick={() => void onDelete(model.id)}>
                        Delete
                      </button>
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
              ) : null}
            </div>
          );
        })}
      </div>
      {download?.message ? <p className="muted">{download.message}</p> : null}
    </>
  );
}
