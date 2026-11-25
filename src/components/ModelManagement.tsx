import { Cpu } from "lucide-react";
import { useState } from "react";
import type {
  ModelDownloadProgress,
  ModelInventory,
  ModelInventoryItem,
} from "../types/dictation";
import { PanelTitle } from "./PanelTitle";

interface ModelManagementProps {
  inventory: ModelInventory | null;
  language: string;
  download: ModelDownloadProgress | null;
  onSelect: (modelId: string, language?: string) => void;
  onDownload: (modelId: string) => Promise<void>;
  onCancelDownload: () => Promise<void>;
  onDelete: (modelId: string) => Promise<void>;
}

const languageNames = new Intl.DisplayNames(["en"], { type: "language" });
const languageLabel = (language: string) =>
  language === "auto" ? "Auto-detect" : languageNames.of(language) ?? language;

function VoiceModelDetails({ model, language }: { model: ModelInventoryItem; language: string }) {
  const speech = model.speech;
  if (!speech) return null;
  const score = speech.benchmark?.accuracy[language];
  const cpu = speech.benchmark?.cpuSeconds;
  const languages = speech.languages.map(languageLabel);
  return (
    <>
      <small>{speech.supportsStreaming ? "Live preview" : "After recording"}</small>
      {speech.description && <small>{speech.description}</small>}
      <small title={languages.join(", ")}>
        {languages.slice(0, 3).join(", ")}
        {languages.length > 3 ? ` + ${languages.length - 3} more` : ""}
      </small>
      {(score || cpu != null) && (
        <dl className="model-benchmarks">
          {score && <div><dt>{languageLabel(language)} accuracy</dt>
            <dd>{score.grade} ({score.errorPercent.toFixed(2)}% error)</dd></div>}
          {cpu != null && <div><dt>Reference CPU</dt><dd>{cpu.toFixed(2)} s / 30 s of audio</dd></div>}
        </dl>
      )}
      <a href={speech.sourceUrl} target="_blank" rel="noreferrer">Model card</a>
    </>
  );
}

export function ModelManagement({
  inventory,
  language,
  download,
  onSelect,
  onDownload,
  onCancelDownload,
  onDelete,
}: ModelManagementProps) {
  const downloadInProgress = Boolean(download &&
    ["starting", "downloading", "verifying"].includes(download.status));
  const [query, setQuery] = useState("");
  const [languageFilter, setLanguageFilter] = useState("");
  const [sort, setSort] = useState("recommended");
  const models = inventory?.models ?? [];
  const activeModel = models.find((model) => model.id === inventory?.activeModelId);
  const activeLanguages = activeModel?.speech?.languages ?? [];
  const canAutoDetect = activeModel?.speech?.supportsLanguageDetection;
  const languageChoices = [...new Set(models.flatMap((model) => model.speech?.languages ?? []))]
    .sort((left, right) => languageLabel(left).localeCompare(languageLabel(right)));
  const benchmarkLanguage = languageFilter || (language === "auto" ? "en" : language);
  const search = query.trim().toLowerCase();
  const visible = models.filter((model) =>
    (!languageFilter || model.speech?.languages.includes(languageFilter)) &&
    `${model.label} ${model.id} ${model.speech?.description ?? ""}`.toLowerCase().includes(search),
  ).sort((left, right) => {
    const score = (model: ModelInventoryItem) => {
      switch (sort) {
        case "accuracy": return model.speech?.benchmark?.accuracy[benchmarkLanguage]?.errorPercent ?? Infinity;
        case "speed": return model.speech?.benchmark?.cpuSeconds ?? Infinity;
        case "size": return model.sizeMb ?? Infinity;
        default: return Number(!model.speech?.recommended);
      }
    };
    return score(left) - score(right) ||
      Number(right.id === inventory?.activeModelId) - Number(left.id === inventory?.activeModelId) ||
      Number(right.installed) - Number(left.installed) || left.label.localeCompare(right.label);
  });
  return (
    <>
      <PanelTitle icon={<Cpu size={22} />} title="Voice models" />
      <p className="muted">Models work offline after download. Handy benchmarks use a reference CPU.</p>
      {activeModel?.installed && (activeLanguages.length > 1 || canAutoDetect) && (
        <label className="speech-language-control">
          Dictation language
          <select
            value={language}
            disabled={downloadInProgress}
            onChange={(event) => onSelect(activeModel.id, event.currentTarget.value)}
          >
            {canAutoDetect && <option value="auto">Auto-detect</option>}
            {activeLanguages.map((tag) => <option key={tag} value={tag}>{languageLabel(tag)}</option>)}
          </select>
        </label>
      )}
      <div className="speech-model-filters">
        <input
          type="search"
          aria-label="Search voice models"
          placeholder="Search voice models"
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <select
          aria-label="Filter models by language"
          value={languageFilter}
          onChange={(event) => setLanguageFilter(event.currentTarget.value)}
        >
          <option value="">All languages</option>
          {languageChoices.map((tag) => <option key={tag} value={tag}>{languageLabel(tag)}</option>)}
        </select>
        <select aria-label="Sort models" value={sort} onChange={(event) => setSort(event.currentTarget.value)}>
          <option value="recommended">Recommended</option>
          <option value="size">Smallest download</option>
          <option value="speed">Fastest on CPU</option>
          <option value="accuracy">Lowest error rate</option>
        </select>
      </div>
      {inventory && visible.length === 0 && <p className="muted">No models match these filters.</p>}
      <div className="model-grid speech-model-grid">
        {visible.map((model) => {
          const active = inventory?.activeModelId === model.id;
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
              <VoiceModelDetails model={model} language={benchmarkLanguage} />
              <div className="model-pill__actions">
                {downloading ? (
                  <button type="button" className="button button--ghost" onClick={() => void onCancelDownload()}>
                    Cancel {download?.percent != null ? `${Math.round(download.percent)}%` : ""}
                  </button>
                ) : model.installed ? (
                  <>
                    <button
                      type="button"
                      className="button button--ghost"
                      disabled={active || downloadInProgress}
                      onClick={() => onSelect(model.id, languageFilter || undefined)}
                    >
                      {active ? "Active" : "Use"}
                    </button>
                    {!active && (
                      <button
                        type="button"
                        className="button button--ghost"
                        disabled={downloadInProgress}
                        onClick={() => void onDelete(model.id)}
                      >
                        Delete
                      </button>
                    )}
                  </>
                ) : (
                  <button
                    type="button"
                    className="button button--ghost"
                    disabled={downloadInProgress}
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
      {download?.message && <p className="muted" role="status">{download.message}</p>}
      <a href="https://models.handy.computer/" target="_blank" rel="noreferrer">Handy benchmark source</a>
    </>
  );
}
