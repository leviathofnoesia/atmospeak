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

export function ModelManagement({
  settings,
  inventory,
  download,
  onSelect,
  onDownload,
  onCancelDownload,
  onDelete,
}: ModelManagementProps) {
  return (
    <>
      <PanelTitle icon={<Cpu size={22} />} title="Voice models" />
      <p className="muted">
        Balanced is bundled. Larger optional models stay on this device and can be removed anytime.
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
